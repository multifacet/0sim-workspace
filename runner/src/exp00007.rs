//! Run a workload in simulation and collect stats on fragmentation via `/proc/buddyinfo`.
//!
//! Requires `setup00000`.

use clap::clap_app;

use spurs::{
    cmd,
    ssh::{Execute, SshShell},
    util::escape_for_bash,
};

use crate::common::{
    exp00000::*, output::OutputManager, RESEARCH_WORKSPACE_PATH, ZEROSIM_BENCHMARKS_DIR,
    ZEROSIM_EXPERIMENTS_SUBMODULE,
};
use crate::settings;

/// The amount of time (in hours) to let the NAS CG workload run.
const NAS_CG_HOURS: u64 = 6;

/// The number of iterations for `memhog`.
const MEMHOG_R: usize = 10;

#[derive(Copy, Clone, Debug)]
enum Workload {
    Memcached,
    Cg,
    Memhog,
}

impl Workload {
    pub fn to_str(&self) -> &str {
        match self {
            Workload::Memcached => "memcached_gen_data",
            Workload::Cg => "nas_cg",
            Workload::Memhog => "memhog",
        }
    }

    pub fn from_str(s: &str) -> Self {
        match s {
            "memcached_gen_data" => Workload::Memcached,
            "nas_cg" => Workload::Cg,
            "memhog" => Workload::Memhog,
            _ => panic!("unknown workload: {:?}", s),
        }
    }
}

pub fn cli_options() -> clap::App<'static, 'static> {
    fn is_usize(s: String) -> Result<(), String> {
        s.as_str()
            .parse::<usize>()
            .map(|_| ())
            .map_err(|e| format!("{:?}", e))
    }

    clap_app! { exp00007 =>
        (about: "Run experiment 00007. Requires `sudo`.")
        (@arg HOSTNAME: +required +takes_value
         "The domain name of the remote (e.g. c240g2-031321.wisc.cloudlab.us:22)")
        (@arg USERNAME: +required +takes_value
         "The username on the remote (e.g. markm)")
        (@arg INTERVAL: +required +takes_value {is_usize}
         "The interval at which to collect stats (seconds)")
        (@group WORKLOAD =>
            (@attributes +required)
            (@arg memcached: -m "Run the memcached workload")
            (@arg cg: -c "Run the NAS Parallel Benchmark CG workload")
            (@arg memhog: -h "Run the memhog workload")
        )
        (@arg WARMUP: -w --warmup
         "Pass this flag to warmup the VM before running the main workload.")
        (@arg VMSIZE: +takes_value {is_usize} --vm_size
         "The number of GBs of the VM (defaults to 2048)")
        (@arg CORES: +takes_value {is_usize} -C --cores
         "The number of cores of the VM (defaults to 1)")
    }
}

pub fn run(dry_run: bool, sub_m: &clap::ArgMatches<'_>) -> Result<(), failure::Error> {
    let login = Login {
        username: Username(sub_m.value_of("USERNAME").unwrap()),
        hostname: sub_m.value_of("HOSTNAME").unwrap(),
        host: sub_m.value_of("HOSTNAME").unwrap(),
    };
    let interval = sub_m
        .value_of("INTERVAL")
        .unwrap()
        .parse::<usize>()
        .unwrap();

    let workload = if sub_m.is_present("memcached") {
        Workload::Memcached
    } else if sub_m.is_present("cg") {
        Workload::Cg
    } else if sub_m.is_present("memhog") {
        Workload::Memhog
    } else {
        panic!("unknown workload")
    };

    let vm_size = if let Some(vm_size) = sub_m
        .value_of("VMSIZE")
        .map(|value| value.parse::<usize>().unwrap())
    {
        vm_size
    } else {
        // NAS class E is ~2TB
        2048
    };

    let cores = if let Some(cores) = sub_m
        .value_of("CORES")
        .map(|value| value.parse::<usize>().unwrap())
    {
        cores
    } else {
        VAGRANT_CORES
    };

    let warmup = sub_m.is_present("WARMUP");

    let ushell = SshShell::with_default_key(&login.username.as_str(), &login.host)?;
    let local_git_hash = crate::common::local_research_workspace_git_hash()?;
    let remote_git_hash = crate::common::research_workspace_git_hash(&ushell)?;
    let remote_research_settings = crate::common::get_remote_research_settings(&ushell)?;

    let settings = settings! {
        * workload: format!("fragmentation_{}", workload.to_str()),
        exp: 00007,

        calibrated: false,
        warmup: warmup,

        * vm_size: vm_size,
        * cores: cores,

        stats_interval: interval,

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
    let workload = Workload::from_str(&settings.get::<&str>("workload")[14..]);
    let interval = settings.get::<usize>("stats_interval");
    let vm_size = settings.get::<usize>("vm_size");
    let cores = settings.get::<usize>("cores");
    let calibrate = settings.get::<bool>("calibrated");
    let warmup = settings.get::<bool>("warmup");
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

    let zerosim_exp_path = &format!(
        "/home/vagrant/{}/{}",
        RESEARCH_WORKSPACE_PATH, ZEROSIM_EXPERIMENTS_SUBMODULE
    );
    let zerosim_bmk_path = &format!(
        "/home/vagrant/{}/{}",
        RESEARCH_WORKSPACE_PATH, ZEROSIM_BENCHMARKS_DIR
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
        "echo '{}' > {}/{}",
        escape_for_bash(&params),
        VAGRANT_RESULTS_DIR,
        params_file
    ))?;

    // Warm up
    if warmup {
        const WARM_UP_PATTERN: &str = "-z";
        time!(
            timers,
            "Warmup",
            vshell.run(
                cmd!(
                    "sudo ./target/release/time_mmap_touch {} {} > /dev/null",
                    (vm_size << 30) >> 12,
                    WARM_UP_PATTERN,
                )
                .cwd(zerosim_exp_path)
                .use_bash(),
            )?
        );
    }

    // Record buddyinfo on the guest until signalled to stop.
    let buddyinfo_handle = vshell.spawn(
        cmd!(
            "rm /tmp/exp-stop ; \
             while [ ! -e /tmp/exp-stop ] ; do \
             cat /proc/buddyinfo | tee -a {}/{} ; \
             sleep {} ; \
             done",
            VAGRANT_RESULTS_DIR,
            output_file,
            interval
        )
        .use_bash(),
    )?;

    // Run the actual workload
    match workload {
        Workload::Memcached => {
            // Start server
            vshell.run(cmd!("memcached -m {} -d -u vagrant", vm_size * 1024))?;

            // Start workload
            time!(
                timers,
                "Workload",
                vshell.run(
                    cmd!(
                        "./target/release/memcached_gen_data localhost:11211 {} > /dev/null",
                        vm_size,
                    )
                    .cwd(zerosim_exp_path)
                )?
            );
        }

        Workload::Cg => {
            time!(timers, "Workload", {
                let _ = vshell.spawn(
                    cmd!("./bin/cg.E.x > {}/{}", VAGRANT_RESULTS_DIR, output_file)
                        .cwd(&format!("{}/NPB3.4/NPB3.4-OMP", zerosim_bmk_path)),
                )?;

                std::thread::sleep(std::time::Duration::from_secs(3600 * NAS_CG_HOURS));
            });
        }

        Workload::Memhog => {
            time!(
                timers,
                "Workload",
                vshell.run(cmd!("memhog -r{} {}g > /dev/null", MEMHOG_R, vm_size,))?
            );
        }
    }

    vshell.run(cmd!("touch /tmp/exp-stop"))?;
    time!(
        timers,
        "Waiting for buddyinfo thread to halt",
        buddyinfo_handle.join()?
    );

    ushell.run(cmd!("date"))?;

    vshell.run(cmd!(
        "echo -e '{}' > {}/{}",
        crate::common::timings_str(timers.as_slice()),
        VAGRANT_RESULTS_DIR,
        time_file
    ))?;

    Ok(())
}