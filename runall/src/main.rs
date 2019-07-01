use std::fs::OpenOptions;
use std::io::{self, BufRead, BufReader, Write};
use std::process::Stdio;
use std::sync::{Arc, Mutex};
use std::time;

use clap::clap_app;

use indicatif::{HumanDuration, MultiProgress, ProgressBar, ProgressStyle};

const RUNNER: &str = "/nobackup/research-workspace/runner/";
const RESULTS_DIR: &str = "/u/m/a/markm/private/large_mem/results/scratch/";

fn main() -> Result<(), failure::Error> {
    let matches = clap_app! { runall =>
        (about: "Run the given commands on the given machines, copying\
                 the results to the result file on this machine.")
        (@arg MACHINES: +required +takes_value
         "A file with the machines (IP:PORT) to use, one per line, \
          all capable of running any experiment")
        (@arg EXPERIMENTS: +required +takes_value
         "A file with the runner subcommands for all experiments, one per line, \
          using the string `{MACHINE}` for the machine name. HACK: the command \
          should not have whitespace except between arguments.")
    }
    .get_matches();

    let machines_file = matches.value_of("MACHINES").unwrap();
    let cmds_file = matches.value_of("EXPERIMENTS").unwrap();

    let machines = parse_machines(machines_file)?;
    let cmds = parse_cmds(cmds_file)?;

    do_work(machines, cmds)?;

    Ok(())
}

fn parse_machines(machines_file: &str) -> Result<Vec<String>, failure::Error> {
    Ok(std::fs::read_to_string(machines_file)?
        .lines()
        .map(str::to_owned)
        .collect())
}

fn parse_cmds(cmds_file: &str) -> Result<Vec<String>, failure::Error> {
    Ok(std::fs::read_to_string(cmds_file)?
        .lines()
        .map(str::to_owned)
        .collect())
}

fn do_work(machines: Vec<String>, cmds: Vec<String>) -> Result<(), failure::Error> {
    let spinner_style = ProgressStyle::default_spinner().template("{prefix:.bold.dim} {wide_msg}");
    let m = MultiProgress::new();

    let start_time = time::Instant::now();
    let ncmds = cmds.len();

    // Use this as a stack of free machines.
    let machines = Arc::new(Mutex::new(machines));

    let results = Arc::new(Mutex::new(Vec::new()));

    for (i, cmd) in cmds.into_iter().enumerate() {
        // Each command has 3 steps:
        // 1) Get a machine.
        // 2) Run the command.
        // 3) Copy the results to this host.

        // Progress spinner
        let pb = m.add(ProgressBar::new(1));
        pb.set_style(spinner_style.clone());
        pb.set_prefix(&format!("[{}/{}]", i + 1, ncmds));
        pb.set_message(&format!("[waiting] {}", cmd));

        let machines = Arc::clone(&machines);
        let results = Arc::clone(&results);

        // Spawn a thread for each command.
        let _ = std::thread::spawn(move || {
            // Wait for available machine.
            let mut machine;
            loop {
                if let Some(next_machine) = machines.lock().unwrap().pop() {
                    machine = next_machine;
                    break;
                }

                // Sleep a bit and wait for the next machine.
                std::thread::sleep(time::Duration::from_secs(5));
            }

            pb.set_message(&format!("[running] {} > {}", machine, cmd));

            // Run cmd and get results path
            let cmd_results = run_cmd(&machine, &cmd);

            let results_str = match cmd_results {
                // Need to copy results
                Ok(Some(results_path)) => {
                    pb.set_message(&format!("[copying results] {} > {}", machine, cmd));

                    // HACK: assume all machine names are in the form HOSTNAME:PORT
                    let machine_ip = machine.split(":").next().unwrap();

                    let scp_result = std::process::Command::new("scp")
                        .arg(&format!("{}:{}", machine_ip, results_path))
                        .arg(&format!("{}", RESULTS_DIR))
                        .stdout(Stdio::null())
                        .stderr(Stdio::null())
                        .output();

                    match scp_result {
                        Ok(..) => pb.finish_with_message(&format!(
                            "[done ({})] {} > {}",
                            HumanDuration(time::Instant::now() - start_time),
                            machine,
                            cmd
                        )),
                        Err(e) => pb.finish_with_message(&format!(
                            "[done. scp error. ({})] {} {} > {}",
                            HumanDuration(time::Instant::now() - start_time),
                            e,
                            machine,
                            cmd
                        )),
                    }

                    results_path
                }

                Ok(None) => {
                    pb.finish_with_message(&format!(
                        "[done. no output. ({})] {} > {}",
                        HumanDuration(time::Instant::now() - start_time),
                        machine,
                        cmd
                    ));

                    "NONE".into()
                }

                Err(e) => {
                    pb.finish_with_message(&format!(
                        "[FAILED ({})] {} {} > {}",
                        HumanDuration(time::Instant::now() - start_time),
                        e,
                        machine,
                        cmd
                    ));

                    "FAILED".into()
                }
            };

            // Release the machine
            machines.lock().unwrap().push(machine);

            results.lock().unwrap().push((cmd, results_str));
        });
    }

    m.join().unwrap();

    for (cmd, path) in results.lock().unwrap().iter() {
        println!("{} {}", cmd, path);
    }

    Ok(())
}

fn run_cmd(machine: &str, cmd: &str) -> Result<Option<String>, failure::Error> {
    let cmd = cmd.replace("{MACHINE}", &machine);

    // Open a tmp file for the cmd output
    let mut tmp_file = OpenOptions::new()
        .truncate(true)
        .write(true)
        .create(true)
        .open(&format!(
            "/tmp/{}",
            cmd.replace(" ", "_").replace("{", "_").replace("}", "_")
        ))?;

    let stderr_file = OpenOptions::new()
        .truncate(true)
        .write(true)
        .create(true)
        .open(&format!(
            "/tmp/{}",
            cmd.replace(" ", "_").replace("{", "_").replace("}", "_")
        ))?;

    // Run the command, piping output to a buf reader.
    let output = std::process::Command::new("cargo")
        .args(&["run", "--", "--print_results_path"])
        .args(&cmd.split_whitespace().collect::<Vec<_>>())
        .current_dir(RUNNER)
        .stdout(Stdio::piped())
        .stderr(Stdio::from(stderr_file))
        .spawn()?
        .stdout
        .ok_or_else(|| {
            io::Error::new(io::ErrorKind::Other, "Could not capture standard output.")
        })?;

    let reader = BufReader::new(output);

    let mut results_path = None;

    reader
        .lines()
        .filter_map(|line| line.ok())
        .for_each(|line| {
            // Check if there was a results path printed.
            if line.starts_with("RESULTS: ") {
                results_path = Some(line[9..].to_string());
            }

            match writeln!(tmp_file, "{}", line) {
                Ok(..) => {}
                Err(e) => {
                    // Ugly but better than nothing...
                    println!("{} {}", e, line);
                }
            }
        });

    Ok(results_path)
}
