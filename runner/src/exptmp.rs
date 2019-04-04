//! This file is for temporary experiments. If an experiment has long-term value, it should be
//! moved to another file and given an actual experiment number.
//!
//! Requires `setup00000`.

use clap::ArgMatches;

use spurs::{
    cmd,
    ssh::{Execute, SshShell},
    util::escape_for_bash,
};

use crate::common::{
    exp00000::*, output::OutputManager, /* setup00000::CLOUDLAB_SHARED_RESULTS_DIR, */
    RESEARCH_WORKSPACE_PATH, ZEROSIM_EXPERIMENTS_SUBMODULE,
};
use crate::settings;

pub fn run(dry_run: bool, sub_m: &ArgMatches<'_>) -> Result<(), failure::Error> {
    let login = Login {
        username: Username(sub_m.value_of("USERNAME").unwrap()),
        hostname: sub_m.value_of("CLOUDLAB").unwrap(),
        host: sub_m.value_of("CLOUDLAB").unwrap(),
    };
    let size = sub_m.value_of("SIZE").unwrap().parse::<usize>().unwrap();
    let pattern = if sub_m.is_present("memcached") {
        None
    } else {
        Some(if sub_m.is_present("zeros") {
            "-z"
        } else {
            "-c"
        })
    };
    let vm_size = sub_m
        .value_of("VMSIZE")
        .map(|value| value.parse::<usize>().unwrap());
    let cores = sub_m
        .value_of("CORES")
        .map(|value| value.parse::<usize>().unwrap());
    let warmup = sub_m.is_present("WARMUP");

    let vm_size = if let Some(vm_size) = vm_size {
        vm_size
    } else {
        VAGRANT_MEM
    };

    let cores = if let Some(cores) = cores {
        cores
    } else {
        VAGRANT_CORES
    };

    let pf_time = sub_m
        .value_of("PFTIME")
        .map(|s| s.to_string().parse::<u64>().unwrap());

    let ushell = SshShell::with_default_key(&login.username.as_str(), &login.host)?;
    let local_git_hash = crate::common::local_research_workspace_git_hash()?;
    let remote_git_hash = crate::common::research_workspace_git_hash(&ushell)?;
    let remote_research_settings = crate::common::get_remote_research_settings(&ushell)?;

    let settings = settings! {
        * workload: if pattern.is_some() { "time_mmap_touch" } else { "memcached_gen_data" },
        exp: "tmp",

        * size: size,
        pattern: pattern,
        calibrated: false,
        warmup: warmup,
        pf_time: pf_time,

        * vm_size: vm_size,
        cores: cores,

        zswap_max_pool_percent: 50,

        username: login.username.as_str(),
        host: login.hostname,

        local_git_hash: local_git_hash,
        remote_git_hash: remote_git_hash,

        remote_research_settings: remote_research_settings,
    };

    run_inner(dry_run, &login, settings)
}

/// Run the experiment using the settings passed. Note that because the only thing we are passed
/// are the settings, we know that there is no information that is not recorded in the settings
/// file.
fn run_inner<A>(
    dry_run: bool,
    login: &Login<A>,
    settings: OutputManager,
) -> Result<(), failure::Error>
where
    A: std::net::ToSocketAddrs + std::fmt::Display + std::fmt::Debug,
{
    let vm_size = settings.get::<usize>("vm_size");
    let size = settings.get::<usize>("size");
    let cores = settings.get::<usize>("cores");
    let pattern = settings.get::<Option<&str>>("pattern");
    let warmup = settings.get::<bool>("warmup");
    let calibrate = settings.get::<bool>("calibrated");
    let zswap_max_pool_percent = settings.get::<usize>("zswap_max_pool_percent");
    let pf_time = settings.get::<Option<u64>>("pf_time");

    // Reboot
    initial_reboot(dry_run, &login)?;

    // Connect to host
    let mut ushell = connect_and_setup_host_only(dry_run, &login)?;

    // Turn on SSDSWAP.
    turn_on_ssdswap(&ushell, dry_run)?;

    // Start and connect to VM
    let vshell = start_vagrant(&ushell, &login.host, vm_size, cores)?;

    // Environment
    turn_on_zswap(&mut ushell, dry_run)?;

    ushell.run(
        cmd!(
            "echo {} | sudo tee /sys/module/zswap/parameters/max_pool_percent",
            zswap_max_pool_percent
        )
        .use_bash(),
    )?;

    let zerosim_exp_path = &format!(
        "/home/vagrant/{}/{}",
        RESEARCH_WORKSPACE_PATH, ZEROSIM_EXPERIMENTS_SUBMODULE
    );

    // let zerosim_path_host = &format!("{}/{}", RESEARCH_WORKSPACE_PATH, ZEROSIM_KERNEL_SUBMODULE);

    // Calibrate
    if calibrate {
        vshell.run(cmd!("sudo ./target/release/time_calibrate").cwd(zerosim_exp_path))?;
    }

    let (output_file, params_file) = settings.gen_file_names();
    let params = serde_json::to_string(&settings)?;

    vshell.run(cmd!(
        "echo '{}' > {}/{}",
        escape_for_bash(&params),
        VAGRANT_RESULTS_DIR,
        params_file
    ))?;

    // Run memcached or time_touch_mmap
    if let Some(pattern) = pattern {
        // Warm up
        //const WARM_UP_SIZE: usize = 50; // GB
        if warmup {
            const WARM_UP_PATTERN: &str = "-z";
            vshell.run(
                cmd!(
                    "sudo ./target/release/time_mmap_touch {} {} {} > /dev/null",
                    //(WARM_UP_SIZE << 30) >> 12,
                    //WARM_UP_PATTERN,
                    (size << 30) >> 12,
                    WARM_UP_PATTERN,
                    if let Some(pf_time) = pf_time {
                        format!("{}", pf_time)
                    } else {
                        "".into()
                    },
                )
                .cwd(zerosim_exp_path)
                .use_bash(),
            )?;
        }

        // const PERF_MEASURE_TIME: usize = 960; // seconds

        // let perf_output_early = settings.gen_file_name("perfdata0");
        // let spawn_handle0 = ushell.spawn(cmd!(
        //     "sudo taskset -c 3 {}/tools/perf/perf stat -C 0 -I 1000 \
        //      -e 'cycles,cache-misses,dTLB-load-misses,dTLB-store-misses,\
        //      page-faults,context-switches,vmscan:*,kvm:*' -o {}/{} sleep {}",
        //     zerosim_path_host,
        //     CLOUDLAB_SHARED_RESULTS_DIR,
        //     perf_output_early,
        //     PERF_MEASURE_TIME,
        // ))?;

        // Then, run the actual experiment
        vshell.run(
            cmd!(
                "time sudo ./target/release/time_mmap_touch {} {} > {}/{}",
                (size << 30) >> 12,
                pattern,
                VAGRANT_RESULTS_DIR,
                output_file,
            )
            .cwd(zerosim_exp_path)
            .use_bash(),
        )?;

    // let _ = spawn_handle0.join()?;
    } else {
        vshell.run(cmd!(
            "taskset -c 0 memcached -M -m {} -d -u vagrant",
            (size * 1024)
        ))?;

        // We want to use rdtsc as the time source, so find the cpu freq:
        let freq = ushell
            .run(cmd!("lscpu | grep 'CPU max MHz' | grep -oE '[0-9]+' | head -n1").use_bash())?;
        let freq = freq.stdout.trim();

        // // Measure host stats with perf while the workload is running. We measure at the beginning
        // // of the workload and later in the workload after the "cliff".
        // const PERF_MEASURE_TIME: usize = 50; // seconds
        // const PERF_LATE_DELAY_MS: usize = 85 * 1000; // ms

        // let perf_output_early = settings.gen_file_name("perfdata0");
        // let perf_output_late = settings.gen_file_name("perfdata1");

        // let spawn_handle0 = ushell.spawn(cmd!(
        //     "sudo taskset -c 2 {}/tools/perf/perf stat -C 0 -I 1000 \
        //      -e 'cycles,cache-misses,dTLB-load-misses,dTLB-store-misses,\
        //      page-faults,context-switches,vmscan:*,kvm:*' -o {}/{} sleep {}",
        //     zerosim_path_host,
        //     CLOUDLAB_SHARED_RESULTS_DIR,
        //     perf_output_early,
        //     PERF_MEASURE_TIME,
        // ))?;

        // let spawn_handle1 = ushell.spawn(cmd!(
        //     "sudo taskset -c 2 {}/tools/perf/perf stat -C 0 -I 1000 -D {} \
        //      -e 'cycles,cache-misses,dTLB-load-misses,dTLB-store-misses,\
        //      page-faults,context-switches,vmscan:*,kvm:*' -o {}/{} sleep {}",
        //     zerosim_path_host,
        //     PERF_LATE_DELAY_MS,
        //     CLOUDLAB_SHARED_RESULTS_DIR,
        //     perf_output_late,
        //     PERF_MEASURE_TIME,
        // ))?;

        // We allow errors because the memcached -M flag errors on OOM rather than doing an insert.
        // This gives much simpler performance behaviors. memcached uses a large amount of the memory
        // you give it for bookkeeping, rather than user data, so OOM will almost certainly happen.
        vshell.run(
            cmd!(
                "taskset -c 0 ./target/release/memcached_gen_data localhost:11211 {} --freq {} > {}/{}",
                size,
                freq,
                VAGRANT_RESULTS_DIR,
                output_file,
            )
            .cwd(zerosim_exp_path)
            .use_bash()
            .allow_error(),
        )?;

        // let _ = spawn_handle0.join()?;
        // let _ = spawn_handle1.join()?;
    }

    ushell.run(cmd!("date"))?;

    Ok(())
}
