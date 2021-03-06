//! Run the NAS CG class E workload on the remote test machine in simulation and collect
//! compressibility stats and `/proc/vmstat` on guest during it.
//!
//! Requires `setup00000`.

use clap::clap_app;

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
        run_nas_cg, run_time_mmap_touch, NasClass, TimeMmapTouchConfig, TimeMmapTouchPattern,
    },
};

pub fn cli_options() -> clap::App<'static, 'static> {
    fn is_usize(s: String) -> Result<(), String> {
        s.as_str()
            .parse::<usize>()
            .map(|_| ())
            .map_err(|e| format!("{:?}", e))
    }

    clap_app! { exp00005 =>
        (about: "Run experiment 00005. Requires `sudo`.")
        (@arg HOSTNAME: +required +takes_value
         "The domain name of the remote (e.g. c240g2-031321.wisc.cloudlab.us:22)")
        (@arg USERNAME: +required +takes_value
         "The username on the remote (e.g. markm)")
        (@arg DURATION: +takes_value {is_usize} +required
         "The length of time to run the workload in seconds.")
        (@arg WARMUP: -w --warmup
         "Pass this flag to warmup the VM before running the main workload.")
        (@arg VMSIZE: +takes_value {is_usize}
         "The number of GBs of the VM (defaults to 2048)")
        (@arg CORES: +takes_value {is_usize} -C --cores
         "The number of cores of the VM (defaults to 1)")
    }
}

pub fn run(print_results_path: bool, sub_m: &clap::ArgMatches<'_>) -> Result<(), failure::Error> {
    let login = Login {
        username: sub_m.value_of("USERNAME").unwrap(),
        hostname: sub_m.value_of("HOSTNAME").unwrap(),
        host: sub_m.value_of("HOSTNAME").unwrap(),
    };

    let duration = sub_m
        .value_of("DURATION")
        .unwrap()
        .parse::<usize>()
        .unwrap();

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
        // NAS class E is ~2TB
        2048
    };

    let cores = if let Some(cores) = cores {
        cores
    } else {
        VAGRANT_CORES
    };

    let ushell = SshShell::with_default_key(login.username, login.host)?;
    let local_git_hash = crate::common::local_research_workspace_git_hash()?;
    let remote_git_hash = crate::common::research_workspace_git_hash(&ushell)?;
    let remote_research_settings = crate::common::get_remote_research_settings(&ushell)?;

    let settings = settings! {
        * workload: "nas_cg_class_e",
        exp: 5,

        calibrated: false,
        warmup: warmup,

        * vm_size: vm_size,
        * cores: cores,

        duration: duration,

        zswap_max_pool_percent: 50,

        username: login.username,
        host: login.hostname,

        local_git_hash: local_git_hash,
        remote_git_hash: remote_git_hash,

        remote_research_settings: remote_research_settings,
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
    let duration = settings.get::<usize>("duration");
    let vm_size = settings.get::<usize>("vm_size");
    let cores = settings.get::<usize>("cores");
    let warmup = settings.get::<bool>("warmup");
    let calibrate = settings.get::<bool>("calibrated");
    let zswap_max_pool_percent = settings.get::<usize>("zswap_max_pool_percent");

    // Reboot
    initial_reboot(&login)?;

    // Connect to host
    let mut ushell = connect_and_setup_host_only(&login)?;

    // Turn on SSDSWAP.
    turn_on_ssdswap(&ushell)?;

    // Collect timers on VM
    let mut timers = vec![];

    // Start and connect to VM
    let vshell = time!(
        timers,
        "Start VM",
        start_vagrant(
            &ushell,
            &login.host,
            vm_size,
            cores,
            /* fast */ true,
            ZEROSIM_SKIP_HALT,
            ZEROSIM_LAPIC_ADJUST
        )?
    );

    // Environment
    ZeroSim::turn_on_zswap(&mut ushell)?;
    ZeroSim::zswap_max_pool_percent(&ushell, zswap_max_pool_percent)?;

    let zerosim_exp_path = &dir!(
        "/home/vagrant",
        RESEARCH_WORKSPACE_PATH,
        ZEROSIM_EXPERIMENTS_SUBMODULE
    );
    let zerosim_bmk_path = &dir!(
        "/home/vagrant",
        RESEARCH_WORKSPACE_PATH,
        ZEROSIM_BENCHMARKS_DIR
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
                    pages: (vm_size << 30) >> 12,
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

    // Record vmstat on guest
    let vmstat_file = settings.gen_file_name("vmstat");
    let (_shell, _vmstats_handle) = vshell.spawn(
        cmd!(
            "for (( c=1 ; c<={} ; c++ )) ; do \
             cat /proc/vmstat >> {} ; sleep 1 ; done",
            duration,
            dir!(VAGRANT_RESULTS_DIR, vmstat_file)
        )
        .use_bash(),
    )?;

    // The workload takes a very long time, so we only use the first 2 hours (of wall-clock time).
    // We start this thread that collects stats in the background and terminates after the given
    // amount of time. We spawn the workload, but don't wait for it; rather, we wait for this task.
    let zswapstats_file = settings.gen_file_name("zswapstats");
    let (_shell, zswapstats_handle) = ushell.spawn(
        cmd!(
            "for (( c=1 ; c<={} ; c++ )) ; do \
             sudo tail `sudo find  /sys/kernel/debug/zswap/ -type f`\
             >> {} ; sleep 1 ; done",
            duration,
            dir!(HOSTNAME_SHARED_RESULTS_DIR, zswapstats_file)
        )
        .use_bash(),
    )?;

    time!(timers, "Background stats collection", {
        let _ = run_nas_cg(
            &vshell,
            zerosim_bmk_path,
            NasClass::F,
            Some(&dir!(VAGRANT_RESULTS_DIR, output_file)),
            /* eager */ false,
            &mut tctx,
        )?;

        std::thread::sleep(std::time::Duration::from_secs(duration as u64));

        zswapstats_handle.join()?
    });

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
