//! Run a workload in simulation and collect stats on swapping via `/proc/swap_instrumentation`.
//! The workload can be invoked either to provoke kswapd or direct reclaim.
//!
//! Requires `setup00000`. Requires `setup00001` with the `markm_instrument_swap` branch.

use clap::clap_app;

use spurs::{
    cmd,
    ssh::{Execute, SshShell},
    util::escape_for_bash,
};

use crate::{
    common::{
        exp_0sim::*,
        output::OutputManager,
        paths::{setup00000::*, setup00001::*, *},
    },
    settings,
    workloads::{
        run_memcached_gen_data, run_memhog, run_nas_cg, MemcachedWorkloadConfig, MemhogOptions,
        NasClass,
    },
};

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
    fn is_isize(s: String) -> Result<(), String> {
        s.as_str()
            .parse::<isize>()
            .map(|_| ())
            .map_err(|e| format!("{:?}", e))
    }

    fn is_usize(s: String) -> Result<(), String> {
        s.as_str()
            .parse::<usize>()
            .map(|_| ())
            .map_err(|e| format!("{:?}", e))
    }

    clap_app! { exp00008 =>
        (about: "Run experiment 00008. Requires `sudo`.")
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
        (@arg FACTOR: +takes_value {is_isize} -f --factor
         "The reclaim order extra factor (defaults to 0). Can be positive or negative, \
         but the absolute value should be less than MAX_ORDER for the guest kernel.")
    }
}

pub fn run(print_results_path: bool, sub_m: &clap::ArgMatches<'_>) -> Result<(), failure::Error> {
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

    let factor = if let Some(factor) = sub_m
        .value_of("FACTOR")
        .map(|value| value.parse::<isize>().unwrap())
    {
        factor
    } else {
        0
    };

    let warmup = sub_m.is_present("WARMUP");

    let ushell = SshShell::with_default_key(&login.username.as_str(), &login.host)?;
    let local_git_hash = crate::common::local_research_workspace_git_hash()?;
    let remote_git_hash = crate::common::research_workspace_git_hash(&ushell)?;
    let remote_research_settings = crate::common::get_remote_research_settings(&ushell)?;

    let settings = settings! {
        * workload: format!("swap_{}", workload.to_str()),
        exp: 8,

        calibrated: false,
        warmup: warmup,

        * vm_size: vm_size,
        * cores: cores,

        * factor: factor,

        stats_interval: interval,

        zswap_max_pool_percent: 50,

        username: login.username.as_str(),
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
    let workload = Workload::from_str(&settings.get::<&str>("workload")[5..]);
    let interval = settings.get::<usize>("stats_interval");
    let vm_size = settings.get::<usize>("vm_size");
    let cores = settings.get::<usize>("cores");
    let factor = settings.get::<isize>("factor");
    let calibrate = settings.get::<bool>("calibrated");
    let warmup = settings.get::<bool>("warmup");
    let zswap_max_pool_percent = settings.get::<usize>("zswap_max_pool_percent");

    // Reboot
    initial_reboot(&login)?;

    // Connect to host
    let mut ushell = connect_and_setup_host_only(&login)?;

    // Turn on SSDSWAP.
    turn_on_ssdswap(&ushell)?;

    // Collect timers on VM
    let mut timers = vec![];

    // Environment
    turn_on_zswap(&mut ushell)?;

    // Start and connect to VM
    let vshell = time!(
        timers,
        "Start VM",
        start_vagrant(&ushell, &login.host, vm_size, cores, /* fast */ true)?
    );

    // Mount the guest swap file
    vshell.run(cmd!("sudo swapon {}", VAGRANT_GUEST_SWAPFILE))?;

    // Get the amount of memory the guest thinks it has. (KB)
    let mem_avail = {
        let mem_avail = vshell
            .run(cmd!("grep MemAvailable /proc/meminfo | awk '{{print $2}}'").use_bash())?
            .stdout;
        mem_avail.trim().parse::<usize>().unwrap()
    };
    let swap_avail = {
        let swap_avail = vshell
            .run(cmd!("grep SwapFree /proc/meminfo | awk '{{print $2}}'").use_bash())?
            .stdout;
        swap_avail.trim().parse::<usize>().unwrap()
    };

    // Compute a workload size that is large enough to cause reclamation but small enough to not
    // trigger OOM killer.
    let size = mem_avail + (8 * swap_avail / 10); // KB

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
    let guest_mem_file = settings.gen_file_name("guest_mem");
    let params = serde_json::to_string(&settings)?;

    vshell.run(cmd!(
        "echo '{}' > {}",
        escape_for_bash(&params),
        dir!(VAGRANT_RESULTS_DIR, params_file)
    ))?;

    vshell.run(cmd!(
        "cat /proc/meminfo > {}",
        dir!(VAGRANT_RESULTS_DIR, guest_mem_file)
    ))?;

    if factor != 0 {
        vshell.run(cmd!("echo {} | sudo tee /proc/swap_extra_factor", factor))?;
    }

    // Warm up
    if warmup {
        const WARM_UP_PATTERN: &str = "-z";
        time!(
            timers,
            "Warmup",
            vshell.run(
                cmd!(
                    "sudo ./target/release/time_mmap_touch {} {} > /dev/null",
                    size >> 12,
                    WARM_UP_PATTERN,
                )
                .cwd(zerosim_exp_path)
                .use_bash(),
            )?
        );
    }

    // Record swap_instrumentation on the guest until signalled to stop.
    vshell.run(cmd!("rm -f /tmp/exp-stop"))?;

    let vshell2 = connect_to_vagrant_as_root(login.hostname)?;
    let (_shell, buddyinfo_handle) = vshell2.spawn(
        cmd!(
            "while [ ! -e /tmp/exp-stop ] ; do \
             cat /proc/swap_instrumentation | tee -a {} ; \
             sleep {} ; \
             done ; \
             cat /proc/swap_instrumentation | tee -a {} ; \
             echo done measuring",
            dir!(VAGRANT_RESULTS_DIR, output_file.as_str()),
            interval,
            dir!(VAGRANT_RESULTS_DIR, output_file.as_str()),
        )
        .use_bash(),
    )?;

    // Wait to make sure the collection of stats has started
    vshell.run(
        cmd!(
            "while [ ! -e {} ] ; do sleep 1 ; done",
            dir!(VAGRANT_RESULTS_DIR, output_file.as_str()),
        )
        .use_bash(),
    )?;

    let freq = crate::common::get_cpu_freq(&ushell)?;
    let mut tctx = crate::workloads::TasksetCtx::new(cores);

    // Start the hog process and give it all memory... the hope is that this gets oom killed
    // eventually, but not before some reclaim happens.
    vshell.run(cmd!("rm -f /tmp/hog_ready"))?;

    vshell.run(cmd!(
        "(nohup {}/target/release/hog {} &) ; ps",
        dir!(
            "/home/vagrant",
            RESEARCH_WORKSPACE_PATH,
            ZEROSIM_EXPERIMENTS_SUBMODULE
        ),
        size / 4 // pages
    ))?;

    vshell.run(cmd!("ps aux | grep hog"))?;

    // Wait to make sure the hog has started
    vshell.run(cmd!("while [ ! -e /tmp/hog_ready ] ; do sleep 1 ; done",).use_bash())?;

    // Run the actual workload
    match workload {
        Workload::Memcached => {
            // Start workload
            time!(
                timers,
                "Start and Workload",
                run_memcached_gen_data(
                    &vshell,
                    &MemcachedWorkloadConfig {
                        user: "vagrant",
                        exp_dir: zerosim_exp_path,
                        memcached: &dir!(
                            "/home/vagrant",
                            RESEARCH_WORKSPACE_PATH,
                            ZEROSIM_MEMCACHED_SUBMODULE
                        ),
                        server_size_mb: size >> 10,
                        wk_size_gb: size >> 20,
                        freq: Some(freq),
                        allow_oom: false,
                        pf_time: None,
                        output_file: None,
                        eager: false,
                        client_pin_core: tctx.next(),
                        server_pin_core: None,
                    }
                )?
            );
        }

        Workload::Cg => {
            time!(timers, "Workload", {
                let _ = run_nas_cg(
                    &vshell,
                    zerosim_bmk_path,
                    NasClass::E,
                    Some(&dir!(VAGRANT_RESULTS_DIR, output_file)),
                    /* eager */ false,
                    &mut tctx,
                )?;

                std::thread::sleep(std::time::Duration::from_secs(3600 * NAS_CG_HOURS));
            });
        }

        Workload::Memhog => {
            time!(
                timers,
                "Workload",
                run_memhog(
                    &vshell,
                    &dir!(
                        "/home/vagrant",
                        RESEARCH_WORKSPACE_PATH,
                        ZEROSIM_MEMHOG_SUBMODULE
                    ),
                    Some(MEMHOG_R),
                    size,
                    MemhogOptions::empty(),
                    /* eager */ false,
                    &mut tctx,
                )?
            );
        }
    }

    vshell.run(cmd!("touch /tmp/exp-stop"))?;
    time!(
        timers,
        "Waiting for swap_instrumentation thread to halt",
        buddyinfo_handle.join()?
    );

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
