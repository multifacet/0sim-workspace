//! This program runs different routines remotely. Which routine is chosen by passing different
//! command line arguments. certain routines require extra arguments.

// Useful common routines
#[macro_use]
mod common;

// Automates starting common workloads.
mod workloads;

// Setup routines
mod setup00000;
mod setup00001;

mod manual;

// Experiment routines
mod exptmp;

mod exp00000;
mod exp00002;
mod exp00003;
mod exp00004;
mod exp00005;
mod exp00006;
mod exp00007;
mod exp00008;
mod exp00009;
mod exp00010;

fn run() -> Result<(), failure::Error> {
    let matches = clap::App::new("runner")
        .about(
            "This program runs different routines remotely. Which routine is chosen by passing \
             different command line arguments. certain routines require extra arguments.",
        )
        .arg(
            clap::Arg::with_name("PRINT_RESULTS_PATH")
                .long("print_results_path")
                .help("(For experiments) Print the results path as the last line of output."),
        )
        .subcommand(setup00000::cli_options())
        .subcommand(setup00001::cli_options())
        .subcommand(manual::cli_options())
        .subcommand(exptmp::cli_options())
        .subcommand(exp00000::cli_options())
        .subcommand(exp00002::cli_options())
        .subcommand(exp00003::cli_options())
        .subcommand(exp00004::cli_options())
        .subcommand(exp00005::cli_options())
        .subcommand(exp00006::cli_options())
        .subcommand(exp00007::cli_options())
        .subcommand(exp00008::cli_options())
        .subcommand(exp00009::cli_options())
        .subcommand(exp00010::cli_options())
        .setting(clap::AppSettings::SubcommandRequired)
        .setting(clap::AppSettings::DisableVersion)
        .get_matches();

    let print_results_path = matches.is_present("PRINT_RESULTS_PATH");

    match matches.subcommand() {
        ("setup00000", Some(sub_m)) => setup00000::run(sub_m),
        ("setup00001", Some(sub_m)) => setup00001::run(sub_m),

        ("manual", Some(sub_m)) => manual::run(sub_m),

        ("exptmp", Some(sub_m)) => exptmp::run(print_results_path, sub_m),

        ("exp00000", Some(sub_m)) => exp00000::run(print_results_path, sub_m),
        ("exp00002", Some(sub_m)) => exp00002::run(print_results_path, sub_m),
        ("exp00003", Some(sub_m)) => exp00003::run(print_results_path, sub_m),
        ("exp00004", Some(sub_m)) => exp00004::run(print_results_path, sub_m),
        ("exp00005", Some(sub_m)) => exp00005::run(print_results_path, sub_m),
        ("exp00006", Some(sub_m)) => exp00006::run(print_results_path, sub_m),
        ("exp00007", Some(sub_m)) => exp00007::run(print_results_path, sub_m),
        ("exp00008", Some(sub_m)) => exp00008::run(print_results_path, sub_m),
        ("exp00009", Some(sub_m)) => exp00009::run(print_results_path, sub_m),
        ("exp00010", Some(sub_m)) => exp00010::run(print_results_path, sub_m),

        _ => {
            unreachable!();
        }
    }
}

fn main() {
    env_logger::init();

    // If an error occurred, try to print something helpful.
    if let Err(err) = run() {
        // Errors from SSH commands
        let err = match err.downcast::<spurs::SshError>() {
            Ok(err) => {
                println!(
                    "`runner` encountered the following error while \
                     attempting to run a command over SSH: {}",
                    err
                );

                std::process::exit(101);
            }
            Err(err) => err,
        };

        // Any other errors
        println!("`runner` encountered the following error: {:?}", err);
    }
}
