//! Run the `time_loop` or `locality_mem_access` workload on the remote test machine.
//!
//! Requires `setup00000`.

use clap::clap_app;

use serde::{Deserialize, Serialize};

use spurs::{cmd, Execute, SshShell};
use spurs_util::escape_for_bash;

use crate::{
    common::{
        exp_0sim::*,
        output::OutputManager,
        paths::{setup00000::*, *},
    },
    settings,
    workloads::{
        run_locality_mem_access, run_time_loop, run_time_mmap_touch, LocalityMemAccessConfig,
        LocalityMemAccessMode, TimeMmapTouchConfig, TimeMmapTouchPattern,
    },
};

/// Which workload to run?
#[derive(Debug, Copy, Clone, Serialize, Deserialize)]
enum Workload {
    /// `time_loop`
    TimeLoop,

    /// Single-threaded `locality_mem_access`
    LocalityMemAccess,

    /// Multithreaded `locality_mem_access` with the given number of threads
    MtLocalityMemAccess(usize),
}

pub fn cli_options() -> clap::App<'static, 'static> {
    fn is_usize(s: String) -> Result<(), String> {
        s.as_str()
            .parse::<usize>()
            .map(|_| ())
            .map_err(|e| format!("{:?}", e))
    }

    clap_app! { exp00002 =>
        (about: "Run experiment 00002. Requires `sudo`.")
        (@arg HOSTNAME: +required +takes_value
         "The domain name of the remote (e.g. c240g2-031321.wisc.cloudlab.us:22)")
        (@arg USERNAME: +required +takes_value
         "The username on the remote (e.g. markm)")
        (@arg N: +required +takes_value {is_usize}
         "The number of iterations of the workload (e.g. 50000000), preferably \
          divisible by 8 for `locality_mem_access`")
        (@arg VMSIZE: +takes_value {is_usize} -v --vm_size
         "The number of GBs of the VM (defaults to 1024)")
        (@arg CORES: +takes_value {is_usize} -C --cores
         "The number of cores of the VM (defaults to 1)")
        (@arg WARMUP: -w --warmup
         "Pass this flag to warmup the VM before running the main workload.")
        (@group WORKLOAD =>
            (@attributes +required)
            (@arg TIME_LOOP: -t "Run time_loop")
            (@arg LOCALITY: -l "Run locality_mem_access")
            (@arg MTLOCALITY: -L +takes_value {is_usize}
             "Run multithreaded locality_mem_access with the given number of threads")
        )
    }
}

pub fn run(print_results_path: bool, sub_m: &clap::ArgMatches<'_>) -> Result<(), failure::Error> {
    let login = Login {
        username: sub_m.value_of("USERNAME").unwrap(),
        hostname: sub_m.value_of("HOSTNAME").unwrap(),
        host: sub_m.value_of("HOSTNAME").unwrap(),
    };
    let n = sub_m.value_of("N").unwrap().parse::<usize>().unwrap();
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

    let mut nthreads = 1;

    let workload = if sub_m.is_present("TIME_LOOP") {
        Workload::TimeLoop
    } else if sub_m.is_present("LOCALITY") {
        Workload::LocalityMemAccess
    } else if let Some(threads) = sub_m.value_of("MTLOCALITY") {
        let threads = threads.parse().unwrap();
        nthreads = threads;
        Workload::MtLocalityMemAccess(threads)
    } else {
        unreachable!()
    };

    let ushell = SshShell::with_default_key(&login.username, &login.host)?;
    let local_git_hash = crate::common::local_research_workspace_git_hash()?;
    let remote_git_hash = crate::common::research_workspace_git_hash(&ushell)?;
    let remote_research_settings = crate::common::get_remote_research_settings(&ushell)?;

    let settings = settings! {
        * workload: match workload {
            Workload::TimeLoop => "time_loop",
            Workload::LocalityMemAccess => "locality_mem_access",
            Workload::MtLocalityMemAccess(..) => "locality_mem_access",
        },
        exp: 2,

        warmup: warmup,
        calibrated: false,
        * n: n,
        (nthreads > 1) threads: nthreads,

        * vm_size: vm_size,
        cores: cores,

        zswap_max_pool_percent: 50,
        zerosim_drift_threshold: 10_000_000,
        zerosim_delay: 0,

        username: login.username,
        host: login.hostname,

        local_git_hash: local_git_hash,
        remote_git_hash: remote_git_hash,

        remote_research_settings: remote_research_settings,

        // machine readable version for convenience
        workload_mr: workload,
    };

    run_inner(print_results_path, &login, settings)
}

/// Run the experiment using the settings passed. Note that because the only thing we are passed
/// are the settings, we know that there is no information that is not recorded in the settings
/// file.
fn run_inner<A>(
    print_results_path: bool,
    login: &Login<A>,
    settings: OutputManager,
) -> Result<(), failure::Error>
where
    A: std::net::ToSocketAddrs + std::fmt::Display + std::fmt::Debug + Clone,
{
    let vm_size = settings.get::<usize>("vm_size");
    let cores = settings.get::<usize>("cores");
    let warmup = settings.get::<bool>("warmup");
    let calibrate = settings.get::<bool>("calibrated");
    let n = settings.get::<usize>("n");
    let workload = settings.get::<Workload>("workload_mr");
    let zswap_max_pool_percent = settings.get::<usize>("zswap_max_pool_percent");
    let zerosim_drift_threshold = settings.get::<usize>("zerosim_drift_threshold");
    let zerosim_delay = settings.get::<usize>("zerosim_delay");

    // Reboot
    initial_reboot(&login)?;

    // Collect timers on VM
    let mut timers = vec![];

    // Connect
    let (mut ushell, vshell) = time!(
        timers,
        "Setup host and start VM",
        connect_and_setup_host_and_vagrant(
            &login,
            vm_size,
            cores,
            ZEROSIM_SKIP_HALT,
            ZEROSIM_LAPIC_ADJUST
        )?
    );

    // Environment
    turn_on_zswap(&mut ushell)?;
    set_zerosim_d(&ushell, zerosim_drift_threshold)?;
    set_zerosim_delay(&ushell, zerosim_delay)?;

    ushell.run(
        cmd!(
            "echo {} | sudo tee /sys/module/zswap/parameters/max_pool_percent",
            zswap_max_pool_percent
        )
        .use_bash(),
    )?;

    let zerosim_exp_path = &dir!(
        "/home/vagrant",
        RESEARCH_WORKSPACE_PATH,
        ZEROSIM_EXPERIMENTS_SUBMODULE
    );

    // Calibrate
    if calibrate {
        time!(
            timers,
            "Calibrate",
            vshell.run(cmd!("sudo ./target/release/time_calibrate").cwd(zerosim_exp_path))?
        );
    }

    let (output_file, params_file, time_file, sim_file) = settings.gen_standard_names();
    let params = serde_json::to_string(&settings)?;

    vshell.run(cmd!(
        "echo '{}' > {}",
        escape_for_bash(&params),
        dir!(VAGRANT_RESULTS_DIR, params_file)
    ))?;

    let mut tctx = crate::workloads::TasksetCtx::new(cores);

    // Warm up
    if warmup {
        const WARM_UP_PATTERN: TimeMmapTouchPattern = TimeMmapTouchPattern::Zeros;
        time!(
            timers,
            "Warmup",
            run_time_mmap_touch(
                &vshell,
                &TimeMmapTouchConfig {
                    exp_dir: zerosim_exp_path,
                    pages: ((vm_size << 30) >> 12) >> 1,
                    pattern: WARM_UP_PATTERN,
                    prefault: false,
                    pf_time: None,
                    output_file: None,
                    eager: false,
                    pin_core: tctx.next(),
                }
            )?
        );
    }

    // Then, run the actual experiment
    match workload {
        Workload::TimeLoop => {
            time!(
                timers,
                "Workload",
                run_time_loop(
                    &vshell,
                    zerosim_exp_path,
                    n,
                    &dir!(VAGRANT_RESULTS_DIR, output_file),
                    /* eager */ false,
                    &mut tctx,
                )?
            );
        }

        Workload::LocalityMemAccess => {
            let local_file = settings.gen_file_name("local");
            let nonlocal_file = settings.gen_file_name("nonlocal");

            time!(timers, "Workload", {
                run_locality_mem_access(
                    &vshell,
                    &LocalityMemAccessConfig {
                        exp_dir: zerosim_exp_path,
                        locality: LocalityMemAccessMode::Local,
                        n: n,
                        threads: None,
                        output_file: &dir!(VAGRANT_RESULTS_DIR, local_file),
                        eager: false,
                    },
                )?;
                run_locality_mem_access(
                    &vshell,
                    &LocalityMemAccessConfig {
                        exp_dir: zerosim_exp_path,
                        locality: LocalityMemAccessMode::Random,
                        n: n,
                        threads: None,
                        output_file: &dir!(VAGRANT_RESULTS_DIR, nonlocal_file),
                        eager: false,
                    },
                )?;
            });
        }

        Workload::MtLocalityMemAccess(threads) => {
            let local_file = settings.gen_file_name("local");
            let nonlocal_file = settings.gen_file_name("nonlocal");

            time!(timers, "Workload", {
                run_locality_mem_access(
                    &vshell,
                    &LocalityMemAccessConfig {
                        exp_dir: zerosim_exp_path,
                        locality: LocalityMemAccessMode::Local,
                        n: n,
                        threads: Some(threads),
                        output_file: &dir!(VAGRANT_RESULTS_DIR, local_file),
                        eager: false,
                    },
                )?;
                run_locality_mem_access(
                    &vshell,
                    &LocalityMemAccessConfig {
                        exp_dir: zerosim_exp_path,
                        locality: LocalityMemAccessMode::Random,
                        n: n,
                        threads: Some(threads),
                        output_file: &dir!(VAGRANT_RESULTS_DIR, nonlocal_file),
                        eager: false,
                    },
                )?;
            });
        }
    }

    ushell.run(cmd!("date"))?;

    vshell.run(cmd!(
        "echo -e '{}' > {}",
        crate::common::timings_str(timers.as_slice()),
        dir!(VAGRANT_RESULTS_DIR, time_file)
    ))?;

    crate::common::exp_0sim::gen_standard_sim_output(&sim_file, &ushell, &vshell)?;

    if print_results_path {
        let glob = settings.gen_file_name("*");
        println!("RESULTS: {}", glob);
    }

    Ok(())
}
