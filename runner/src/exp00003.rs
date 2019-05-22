//! Run a memcached workload on the remote test machine designed to induce THP compaction
//! remotely. Measure the number of per-page operations done and undone.
//!
//! Requires `setup00000` followed by `setup00001`.

use spurs::{
    cmd,
    ssh::{Execute, SshShell},
    util::escape_for_bash,
};

use crate::common::{
    exp00003::*, output::OutputManager, RESEARCH_WORKSPACE_PATH, ZEROSIM_EXPERIMENTS_SUBMODULE,
};
use crate::settings;

/// Interval at which to collect thp stats
const INTERVAL: usize = 60; // seconds

pub fn run<A>(
    dry_run: bool,
    login: &Login<A>,
    size: usize,    // GB
    vm_size: usize, // GB
    cores: Option<usize>,
) -> Result<(), failure::Error>
where
    A: std::net::ToSocketAddrs + std::fmt::Display + std::fmt::Debug,
{
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
        * workload: "memcached_per_page_thp_ops",
        exp: 00003,

        * size: size,
        calibrated: false,

        * vm_size: vm_size,
        cores: cores,

        zswap_max_pool_percent: 50,

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

    run_inner(dry_run, login, settings)
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
    let calibrate = settings.get::<bool>("calibrated");
    let zswap_max_pool_percent = settings.get::<usize>("zswap_max_pool_percent");
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
    let (mut ushell, vshell) = connect_and_setup_host_and_vagrant(dry_run, &login, vm_size, cores)?;

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

    // Turn on compaction and force it too happen
    vshell.run(
        cmd!(
            "echo {} | sudo tee /sys/kernel/mm/transparent_hugepage/enabled",
            transparent_hugepage_enabled
        )
        .use_bash(),
    )?;
    vshell.run(
        cmd!(
            "echo {} | sudo tee /sys/kernel/mm/transparent_hugepage/defrag",
            transparent_hugepage_defrag
        )
        .use_bash(),
    )?;
    vshell.run(
        cmd!(
            "echo {} | sudo tee /sys/kernel/mm/transparent_hugepage/khugepaged/defrag",
            transparent_hugepage_khugepaged_defrag
        )
        .use_bash(),
    )?;
    vshell.run(
        cmd!("echo {} | sudo tee /sys/kernel/mm/transparent_hugepage/khugepaged/alloc_sleep_millisecs",
             transparent_hugepage_khugepaged_alloc_sleep_ms).use_bash(),
    )?;
    vshell.run(
        cmd!("echo {} | sudo tee /sys/kernel/mm/transparent_hugepage/khugepaged/scan_sleep_millisecs",
             transparent_hugepage_khugepaged_scan_sleep_ms).use_bash(),
    )?;

    // Run memcached. We need to make it take slightly less memory than the VM, or it will OOM.
    vshell.run(cmd!("memcached -m {} -d -u vagrant", size * 1024))?;

    vshell.run(
        cmd!(
            "./target/release/memcached_and_capture_thp localhost:11211 {} {} > {}/{}",
            size,
            INTERVAL,
            VAGRANT_RESULTS_DIR,
            output_file,
        )
        .cwd(zerosim_exp_path)
        .use_bash()
        .allow_error(),
    )?;

    ushell.run(cmd!("date"))?;

    Ok(())
}
