//! Run a memcached workload on the remote cloudlab machine designed to induce THP compaction
//! remotely. Measure the number of per-page operations done and undone. Unlike exp00003, run
//! this on the bare-metal host, rather than in a VM.
//!
//! Requires `setup00000` and `setup00002`.

use spurs::{
    cmd,
    ssh::{Execute, SshShell},
    util::escape_for_bash,
};

use crate::common::{
    exp00004::*, output::OutputManager, RESEARCH_WORKSPACE_PATH, ZEROSIM_EXPERIMENTS_SUBMODULE,
};
use crate::settings;

const BARE_METAL_RESULTS_DIR: &str = "vm_shared/results/";

/// Interval at which to collect thp stats
const INTERVAL: usize = 60; // seconds

pub fn run<A>(
    dry_run: bool,
    login: &Login<A>,
    size: usize, // GB
) -> Result<(), failure::Error>
where
    A: std::net::ToSocketAddrs + std::fmt::Display + std::fmt::Debug,
{
    let ushell = SshShell::with_default_key(&login.username.as_str(), &login.host)?;
    let git_hash = crate::common::research_workspace_git_hash(&ushell)?;

    let settings = settings! {
        git_hash: git_hash,
        exp: 00004,
        local_git_hash: crate::common::local_research_workspace_git_hash(),

        workload: "memcached_thp_ops_per_page_bare_metal",
        * size: size,

        transparent_hugepage_enabled: "always",
        transparent_hugepage_defrag: "always",
        transparent_hugepage_khugepaged_defrag: 1,
        transparent_hugepage_khugepaged_alloc_sleep_ms: 1000,
        transparent_hugepage_khugepaged_scan_sleep_ms: 1000,

        username: login.username.as_str(),
        host: login.hostname,
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

    let user_home = &format!("/users/{}/", login.username.as_str());
    let zerosim_exp_path = &format!(
        "{}/{}/{}",
        user_home, RESEARCH_WORKSPACE_PATH, ZEROSIM_EXPERIMENTS_SUBMODULE
    );

    let (output_file, params_file) = settings.gen_file_names();
    let params = serde_json::to_string(&settings)?;

    ushell.run(cmd!(
        "echo {} > {}/{}/{}",
        escape_for_bash(&params),
        user_home,
        BARE_METAL_RESULTS_DIR,
        params_file
    ))?;

    ushell.run(cmd!("sudo swapon /dev/sda3"))?;

    // Turn on compaction and force it to happen
    ushell.run(
        cmd!(
            "echo {} | sudo tee /sys/kernel/mm/transparent_hugepage/enabled",
            transparent_hugepage_enabled
        )
        .use_bash(),
    )?;
    ushell.run(
        cmd!(
            "echo {} | sudo tee /sys/kernel/mm/transparent_hugepage/defrag",
            transparent_hugepage_defrag
        )
        .use_bash(),
    )?;
    ushell.run(
        cmd!(
            "echo {} | sudo tee /sys/kernel/mm/transparent_hugepage/khugepaged/defrag",
            transparent_hugepage_khugepaged_defrag
        )
        .use_bash(),
    )?;
    ushell.run(
        cmd!("echo {} | sudo tee /sys/kernel/mm/transparent_hugepage/khugepaged/alloc_sleep_millisecs",
             transparent_hugepage_khugepaged_alloc_sleep_ms).use_bash(),
    )?;
    ushell.run(
        cmd!("echo {} | sudo tee /sys/kernel/mm/transparent_hugepage/khugepaged/scan_sleep_millisecs",
             transparent_hugepage_khugepaged_scan_sleep_ms).use_bash(),
    )?;

    // Run memcached. We need to make it take slightly less memory than RAM + swap, or it will OOM.
    ushell.run(cmd!("memcached -m {} -d", size * 1024))?;

    ushell.run(
        cmd!(
            "./target/release/memcached_and_capture_thp localhost:11211 {} {} > {}/{}",
            size,
            INTERVAL,
            BARE_METAL_RESULTS_DIR,
            output_file,
        )
        .cwd(zerosim_exp_path)
        .use_bash()
        .allow_error(),
    )?;

    ushell.run(cmd!("date"))?;

    ushell.run(cmd!("free -h"))?;

    Ok(())
}
