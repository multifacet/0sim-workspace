//! Run the time_loop workload on the remote cloudlab machine.
//!
//! Requires `setup00000`.

use spurs::{
    cmd,
    ssh::{Execute, SshShell},
    util::escape_for_bash,
};

use crate::common::{
    exp00002::*, output::OutputManager, RESEARCH_WORKSPACE_PATH, ZEROSIM_EXPERIMENTS_SUBMODULE,
};
use crate::settings;

pub fn run<A>(
    dry_run: bool,
    login: &Login<A>,
    n: usize,
    vm_size: Option<usize>, // GB
    cores: Option<usize>,
    warmup: bool,
) -> Result<(), failure::Error>
where
    A: std::net::ToSocketAddrs + std::fmt::Display + std::fmt::Debug,
{
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
        * workload: "time_loop",
        exp: 00002,

        warmup: warmup,
        calibrated: false,
        * n: n,

        * vm_size: vm_size,
        cores: cores,

        zswap_max_pool_percent: 50,

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
    let cores = settings.get::<usize>("cores");
    let warmup = settings.get::<bool>("warmup");
    let calibrate = settings.get::<bool>("calibrated");
    let n = settings.get::<usize>("n");
    let zswap_max_pool_percent = settings.get::<usize>("zswap_max_pool_percent");

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

    // Warm up
    if warmup {
        const WARM_UP_PATTERN: &str = "-z";
        vshell.run(
            cmd!(
                "sudo ./target/release/time_mmap_touch {} {} > /dev/null",
                ((vm_size << 30) >> 12) >> 1,
                WARM_UP_PATTERN,
            )
            .cwd(zerosim_exp_path)
            .use_bash(),
        )?;
    }

    // Then, run the actual experiment
    vshell.run(
        cmd!(
            "sudo ./target/release/time_loop {} > {}/{}",
            n,
            VAGRANT_RESULTS_DIR,
            output_file,
        )
        .cwd(zerosim_exp_path)
        .use_bash(),
    )?;

    ushell.run(cmd!("date"))?;

    Ok(())
}
