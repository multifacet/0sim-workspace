//! Run a memcached workload on the remote test machine designed to induce THP compaction
//! remotely. Measure the number of per-page operations done and undone. Unlike exp00003, run
//! this on the bare-metal host, rather than in a VM.
//!
//! Requires `setup00000` and `setup00002`.

use clap::clap_app;

use spurs::{
    cmd,
    ssh::{Execute, SshShell},
    util::escape_for_bash,
};

use crate::{
    common::{exp_0sim::*, get_user_home_dir, output::OutputManager, paths::*},
    settings,
    workloads::run_memcached_and_capture_thp,
};

const BARE_METAL_RESULTS_DIR: &str = "vm_shared/results/";

/// Interval at which to collect thp stats
const INTERVAL: usize = 60; // seconds

pub fn cli_options() -> clap::App<'static, 'static> {
    fn is_usize(s: String) -> Result<(), String> {
        s.as_str()
            .parse::<usize>()
            .map(|_| ())
            .map_err(|e| format!("{:?}", e))
    }

    clap_app! { exp00004 =>
        (about: "Run experiment 00004. Requires `sudo`.")
        (@arg HOSTNAME: +required +takes_value
         "The domain name of the remote (e.g. c240g2-031321.wisc.cloudlab.us:22)")
        (@arg USERNAME: +required +takes_value
         "The username on the remote (e.g. markm)")
        (@arg SIZE: +required +takes_value {is_usize}
         "The number of GBs of the workload (e.g. 500)")
    }
}

pub fn run(
    dry_run: bool,
    print_results_path: bool,
    sub_m: &clap::ArgMatches<'_>,
) -> Result<(), failure::Error> {
    let login = Login {
        username: Username(sub_m.value_of("USERNAME").unwrap()),
        hostname: sub_m.value_of("HOSTNAME").unwrap(),
        host: sub_m.value_of("HOSTNAME").unwrap(),
    };
    let size = sub_m.value_of("SIZE").unwrap().parse::<usize>().unwrap();

    let ushell = SshShell::with_default_key(&login.username.as_str(), &login.host)?;
    let local_git_hash = crate::common::local_research_workspace_git_hash()?;
    let remote_git_hash = crate::common::research_workspace_git_hash(&ushell)?;
    let remote_research_settings = crate::common::get_remote_research_settings(&ushell)?;

    let settings = settings! {
        * workload: "memcached_thp_ops_per_page_bare_metal",
        exp: 00004,

        * size: size,

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

    run_inner(dry_run, print_results_path, &login, settings)
}

/// Run the experiment using the settings passed. Note that because the only thing we are passed
/// are the settings, we know that there is no information that is not recorded in the settings
/// file.
fn run_inner<A>(
    dry_run: bool,
    print_results_path: bool,
    login: &Login<A>,
    settings: OutputManager,
) -> Result<(), failure::Error>
where
    A: std::net::ToSocketAddrs + std::fmt::Display + std::fmt::Debug + Clone,
{
    let size = settings.get::<usize>("size");
    let transparent_hugepage_enabled = settings.get::<&str>("transparent_hugepage_enabled");
    let transparent_hugepage_defrag = settings.get::<&str>("transparent_hugepage_defrag");
    let transparent_hugepage_khugepaged_defrag =
        settings.get::<usize>("transparent_hugepage_khugepaged_defrag");
    let transparent_hugepage_khugepaged_alloc_sleep_ms =
        settings.get::<usize>("transparent_hugepage_khugepaged_alloc_sleep_ms");
    let transparent_hugepage_khugepaged_scan_sleep_ms =
        settings.get::<usize>("transparent_hugepage_khugepaged_scan_sleep_ms");

    // Reboot
    initial_reboot(dry_run, &login)?;

    // Connect
    let ushell = connect_and_setup_host_only(dry_run, &login)?;

    let user_home = &get_user_home_dir(&ushell)?;
    let zerosim_exp_path = &dir!(
        user_home.as_str(),
        RESEARCH_WORKSPACE_PATH,
        ZEROSIM_EXPERIMENTS_SUBMODULE
    );

    // Collect timers on VM
    let mut timers = vec![];

    let (output_file, params_file) = settings.gen_file_names();
    let time_file = settings.gen_file_name("time");
    let params = serde_json::to_string(&settings)?;

    ushell.run(cmd!(
        "echo '{}' > {}",
        escape_for_bash(&params),
        dir!(user_home.as_str(), BARE_METAL_RESULTS_DIR, params_file)
    ))?;

    ushell.run(cmd!("sudo swapon /dev/sda3"))?;

    // Turn on compaction and force it to happen
    crate::common::turn_on_thp(
        &ushell,
        transparent_hugepage_enabled,
        transparent_hugepage_defrag,
        transparent_hugepage_khugepaged_defrag,
        transparent_hugepage_khugepaged_alloc_sleep_ms,
        transparent_hugepage_khugepaged_scan_sleep_ms,
    )?;

    let cores = crate::common::get_num_cores(&ushell)?;
    let mut tctx = crate::workloads::TasksetCtx::new(cores);

    // Run workload
    time!(
        timers,
        "Setup and Workload",
        run_memcached_and_capture_thp(
            &ushell,
            &crate::workloads::MemcachedWorkloadConfig::default()
                .user(login.username.as_str())
                .exp_dir(zerosim_exp_path)
                .server_size_mb(size << 10)
                .wk_size_gb(size)
                .allow_oom(true)
                .output_file(None)
                .eager(false)
                .client_pin_core(tctx.next())
                .server_pin_core(None),
            INTERVAL,
            /* continual_compaction */ None,
            &dir!(BARE_METAL_RESULTS_DIR, output_file),
        )?
    );

    ushell.run(cmd!("date"))?;

    ushell.run(cmd!("free -h"))?;

    ushell.run(cmd!(
        "echo -e '{}' > {}",
        crate::common::timings_str(timers.as_slice()),
        dir!(BARE_METAL_RESULTS_DIR, time_file)
    ))?;

    if print_results_path {
        let glob = settings.gen_file_name("*");
        println!("RESULTS: {}", glob);
    }

    Ok(())
}
