//! Run a memcached workload on the remote host (in simulation) in the presence of aggressive
//! kernel memory compaction.
//!
//! This workload has two alternative modes:
//! 1) Enable THP compaction and set kcompactd to run aggressively.
//! 2) Induce continual compaction by causing spurious failures in the compaction algo.
//!
//! Run a memcached workload on the remote test machine designed to induce THP compaction remotely.
//! Measure the latency of the workload and the number of per-page operations done and undone.
//!
//! Requires `setup00000` followed by `setup00001`.

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
        paths::{setup00000::*, *},
    },
    settings,
    setup00001::GUEST_SWAP_GBS,
    workloads::{run_memcached_and_capture_thp, MemcachedWorkloadConfig},
};

/// Interval at which to collect thp stats
const INTERVAL: usize = 60; // seconds

pub fn cli_options() -> clap::App<'static, 'static> {
    fn is_usize(s: String) -> Result<(), String> {
        s.as_str()
            .parse::<usize>()
            .map(|_| ())
            .map_err(|e| format!("{:?}", e))
    }

    clap_app! { exp00003 =>
        (about: "Run experiment 00003. Requires `sudo`.")
        (@arg HOSTNAME: +required +takes_value
         "The domain name of the remote (e.g. c240g2-031321.wisc.cloudlab.us:22)")
        (@arg USERNAME: +required +takes_value
         "The username on the remote (e.g. markm)")
        (@arg VMSIZE: +required +takes_value {is_usize}
         "The number of GBs of the VM (e.g. 500)")
        (@arg CORES: -C --cores +takes_value {is_usize}
         "(Optional) The number of cores of the VM (defaults to 1)")
        (@arg SIZE: -s --size +takes_value {is_usize}
         "(Optional) The number of GBs of the workload (e.g. 500). Defaults to VMSIZE + 10")
        (@arg CONTINUAL: --continual_compaction +takes_value {is_usize}
         "(Optional) Enables continual compaction via spurious failures of the given mode")
    }
}

pub fn run(print_results_path: bool, sub_m: &clap::ArgMatches<'_>) -> Result<(), failure::Error> {
    let login = Login {
        username: Username(sub_m.value_of("USERNAME").unwrap()),
        hostname: sub_m.value_of("HOSTNAME").unwrap(),
        host: sub_m.value_of("HOSTNAME").unwrap(),
    };
    let vm_size = sub_m.value_of("VMSIZE").unwrap().parse::<usize>().unwrap();

    let size = if let Some(size) = sub_m
        .value_of("SIZE")
        .map(|value| value.parse::<usize>().unwrap())
    {
        size
    } else {
        // Just a bit smaller so we don't OOM
        vm_size + GUEST_SWAP_GBS - 1
    };

    let cores = if let Some(cores) = sub_m
        .value_of("CORES")
        .map(|value| value.parse::<usize>().unwrap())
    {
        cores
    } else {
        VAGRANT_CORES
    };

    let continual_compaction = sub_m
        .value_of("CONTINUAL")
        .map(|value| value.parse::<usize>().unwrap());

    let ushell = SshShell::with_default_key(&login.username.as_str(), &login.host)?;
    let local_git_hash = crate::common::local_research_workspace_git_hash()?;
    let remote_git_hash = crate::common::research_workspace_git_hash(&ushell)?;
    let remote_research_settings = crate::common::get_remote_research_settings(&ushell)?;

    let settings = settings! {
        * workload: "memcached_per_page_thp_ops",
        * continual_compaction: continual_compaction,
        exp: 3,

        * size: size,
        calibrated: false,

        * vm_size: vm_size,
        cores: cores,

        zswap_max_pool_percent: 50,
        zerosim_d: 10_000_000,
        zerosim_delta: 0,

        transparent_hugepage_enabled: "always",
        transparent_hugepage_defrag: "always",
        transparent_hugepage_khugepaged_defrag: 1,
        transparent_hugepage_khugepaged_alloc_sleep_ms: 1000,
        transparent_hugepage_khugepaged_scan_sleep_ms: 1000,

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
    let vm_size = settings.get::<usize>("vm_size");
    let size = settings.get::<usize>("size");
    let cores = settings.get::<usize>("cores");
    let calibrate = settings.get::<bool>("calibrated");
    let zswap_max_pool_percent = settings.get::<usize>("zswap_max_pool_percent");
    let zerosim_d = settings.get::<usize>("zerosim_d");
    let zerosim_delta = settings.get::<usize>("zerosim_delta");
    let transparent_hugepage_enabled = settings.get::<&str>("transparent_hugepage_enabled");
    let transparent_hugepage_defrag = settings.get::<&str>("transparent_hugepage_defrag");
    let transparent_hugepage_khugepaged_defrag =
        settings.get::<usize>("transparent_hugepage_khugepaged_defrag");
    let transparent_hugepage_khugepaged_alloc_sleep_ms =
        settings.get::<usize>("transparent_hugepage_khugepaged_alloc_sleep_ms");
    let transparent_hugepage_khugepaged_scan_sleep_ms =
        settings.get::<usize>("transparent_hugepage_khugepaged_scan_sleep_ms");
    let continual_compaction = settings.get::<Option<usize>>("continual_compaction");

    // Reboot
    initial_reboot(&login)?;

    // Collect timers on VM
    let mut timers = vec![];

    // Connect
    let (mut ushell, vshell) = time!(
        timers,
        "Setup host and start VM",
        connect_and_setup_host_and_vagrant(&login, vm_size, cores)?
    );

    // Environment
    turn_on_zswap(&mut ushell)?;
    set_zerosim_d(&ushell, zerosim_d)?;
    set_zerosim_delta(&ushell, zerosim_delta)?;

    ushell.run(
        cmd!(
            "echo {} | sudo tee /sys/module/zswap/parameters/max_pool_percent",
            zswap_max_pool_percent
        )
        .use_bash(),
    )?;

    // Mount guest swap space
    let research_settings = crate::common::get_remote_research_settings(&ushell)?;
    let guest_swap: &str =
        crate::common::get_remote_research_setting(&research_settings, "guest_swap")?.unwrap();
    vshell.run(cmd!("sudo swapon {}", guest_swap))?;

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
    let memcached_timing_file = settings.gen_file_name("memcached_latency");
    let params = serde_json::to_string(&settings)?;

    vshell.run(cmd!(
        "echo '{}' > {}",
        escape_for_bash(&params),
        dir!(VAGRANT_RESULTS_DIR, params_file)
    ))?;

    // Turn on compaction and force it too happen
    crate::common::turn_on_thp(
        &vshell,
        transparent_hugepage_enabled,
        transparent_hugepage_defrag,
        transparent_hugepage_khugepaged_defrag,
        transparent_hugepage_khugepaged_alloc_sleep_ms,
        transparent_hugepage_khugepaged_scan_sleep_ms,
    )?;

    let mut tctx = crate::workloads::TasksetCtx::new(cores);

    time!(
        timers,
        "Start and Workload",
        run_memcached_and_capture_thp(
            &vshell,
            &MemcachedWorkloadConfig {
                user: "vagrant",
                exp_dir: zerosim_exp_path,
                memcached: &dir!(
                    "/home/vagrant",
                    RESEARCH_WORKSPACE_PATH,
                    ZEROSIM_MEMCACHED_SUBMODULE
                ),
                server_size_mb: size << 10,
                wk_size_gb: size,
                allow_oom: false,
                output_file: Some(&dir!(VAGRANT_RESULTS_DIR, memcached_timing_file)),
                eager: false,
                client_pin_core: tctx.next(),
                server_pin_core: None,
                freq: None,
                pf_time: None,
            },
            INTERVAL,
            continual_compaction,
            &dir!(VAGRANT_RESULTS_DIR, output_file),
        )?
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
