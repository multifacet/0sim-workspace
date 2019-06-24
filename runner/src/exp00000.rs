//! Run the time_mmap_touch or memcached_gen_data workload on the remote test machine.
//!
//! Requires `setup00000`.

use clap::clap_app;

use spurs::{
    cmd,
    ssh::{Execute, SshShell},
    util::escape_for_bash,
};

use crate::common::{
    exp00000::*, get_cpu_freq, output::OutputManager, Username, RESEARCH_WORKSPACE_PATH,
    ZEROSIM_EXPERIMENTS_SUBMODULE,
};
use crate::settings;
use crate::workloads::{run_memcached_gen_data, run_time_mmap_touch, TimeMmapTouchPattern};

pub fn cli_options() -> clap::App<'static, 'static> {
    fn is_usize(s: String) -> Result<(), String> {
        s.as_str()
            .parse::<usize>()
            .map(|_| ())
            .map_err(|e| format!("{:?}", e))
    }

    clap_app! { exp00000 =>
        (about: "Run experiment 00000. Requires `sudo`.")
        (@arg HOSTNAME: +required +takes_value
         "The domain name of the remote (e.g. c240g2-031321.wisc.cloudlab.us:22)")
        (@arg USERNAME: +required +takes_value
         "The username on the remote (e.g. markm)")
        (@arg SIZE: +required +takes_value {is_usize}
         "The number of GBs of the workload (e.g. 500)")
        (@group PATTERN =>
            (@attributes +required)
            (@arg zeros: -z "Fill pages with zeros")
            (@arg counter: -c "Fill pages with counter values")
            (@arg memcached: -m "Run a memcached workload")
        )
        (@arg VMSIZE: +takes_value {is_usize} -v --vm_size
         "The number of GBs of the VM (defaults to 1024) (e.g. 500)")
        (@arg CORES: +takes_value {is_usize} -C --cores
         "The number of cores of the VM (defaults to 1)")
        (@arg WARMUP: -w --warmup
         "Pass this flag to warmup the VM before running the main workload.")
        (@arg PREFAULT: -p --prefault
         "Pass this flag to prefault memory before running the main workload \
         (ignored for memcached).")
    }
}

pub fn run(dry_run: bool, sub_m: &clap::ArgMatches<'_>) -> Result<(), failure::Error> {
    let login = Login {
        username: Username(sub_m.value_of("USERNAME").unwrap()),
        hostname: sub_m.value_of("HOSTNAME").unwrap(),
        host: sub_m.value_of("HOSTNAME").unwrap(),
    };
    let size = sub_m.value_of("SIZE").unwrap().parse::<usize>().unwrap();
    let pattern = if sub_m.is_present("memcached") {
        None
    } else {
        Some(if sub_m.is_present("zeros") {
            TimeMmapTouchPattern::Zeros
        } else {
            TimeMmapTouchPattern::Counter
        })
    };
    let vm_size = sub_m
        .value_of("VMSIZE")
        .map(|value| value.parse::<usize>().unwrap());
    let cores = sub_m
        .value_of("CORES")
        .map(|value| value.parse::<usize>().unwrap());
    let warmup = sub_m.is_present("WARMUP");
    let prefault = sub_m.is_present("PREFAULT");

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

    let ushell = SshShell::with_default_key(&login.username.as_str(), &login.host)?;
    let local_git_hash = crate::common::local_research_workspace_git_hash()?;
    let remote_git_hash = crate::common::research_workspace_git_hash(&ushell)?;
    let remote_research_settings = crate::common::get_remote_research_settings(&ushell)?;

    let settings = settings! {
        * workload: if pattern.is_some() { "time_mmap_touch" } else { "memcached_gen_data" },
        exp: 00000,

        * size: size,
        pattern: pattern,
        prefault: prefault,
        calibrated: false,
        warmup: warmup,

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
    let pattern = settings.get::<Option<TimeMmapTouchPattern>>("pattern");
    let warmup = settings.get::<bool>("warmup");
    let prefault = settings.get::<bool>("prefault");
    let calibrate = settings.get::<bool>("calibrated");
    let zswap_max_pool_percent = settings.get::<usize>("zswap_max_pool_percent");

    // Reboot
    initial_reboot(dry_run, &login)?;

    // Connect to host
    let mut ushell = connect_and_setup_host_only(dry_run, &login)?;

    // Turn on SSDSWAP.
    turn_on_ssdswap(&ushell, dry_run)?;

    // Collect timers on VM
    let mut timers = vec![];

    // Start and connect to VM
    let vshell = time!(
        timers,
        "Start VM",
        start_vagrant(&ushell, &login.host, vm_size, cores, /* fast */ true)?
    );

    // Environment
    turn_on_zswap(&mut ushell, dry_run)?;

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

    let (output_file, params_file) = settings.gen_file_names();
    let time_file = settings.gen_file_name("time");
    let params = serde_json::to_string(&settings)?;

    vshell.run(cmd!(
        "echo '{}' > {}",
        escape_for_bash(&params),
        dir!(VAGRANT_RESULTS_DIR, params_file)
    ))?;

    // Warm up
    if warmup {
        //const WARM_UP_SIZE: usize = 50; // GB
        const WARM_UP_PATTERN: TimeMmapTouchPattern = TimeMmapTouchPattern::Zeros;
        time!(
            timers,
            "Warmup",
            run_time_mmap_touch(
                &vshell,
                zerosim_exp_path,
                (size << 30) >> 12,
                WARM_UP_PATTERN,
                /* prefault */ false,
                /* pf_time */ None,
                None
            )?
        );
    }

    // We want to use rdtsc as the time source, so find the cpu freq:
    let freq = get_cpu_freq(&ushell)?;

    // Run memcached or time_touch_mmap
    if let Some(pattern) = pattern {
        time!(
            timers,
            "Workload",
            run_time_mmap_touch(
                &vshell,
                zerosim_exp_path,
                (size << 30) >> 12,
                pattern,
                prefault,
                /* pf_time */ None,
                Some(&dir!(VAGRANT_RESULTS_DIR, output_file)),
            )?
        );
    } else {
        time!(
            timers,
            "Workload",
            run_memcached_gen_data(
                &vshell,
                "vagrant",
                zerosim_exp_path,
                size << 10,
                size,
                Some(freq),
                /* allow_oom */ true,
                /* pf_time */ None,
                Some(&dir!(VAGRANT_RESULTS_DIR, output_file))
            )?
        );
    }

    ushell.run(cmd!("date"))?;

    vshell.run(cmd!(
        "echo -e '{}' > {}",
        crate::common::timings_str(timers.as_slice()),
        dir!(VAGRANT_RESULTS_DIR, time_file)
    ))?;

    Ok(())
}
