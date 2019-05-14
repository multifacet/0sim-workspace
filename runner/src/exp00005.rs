//! Run the NAS CG class E workload on the remote cloudlab machine.
//!
//! Requires `setup00000`.

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

pub fn run<A>(
    dry_run: bool,
    login: &Login<A>,
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
        // NAS class E is ~2TB
        2048
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
        * workload: "nas_cg_class_e",
        exp: 00005,

        calibrated: false,
        warmup: warmup,

        * vm_size: vm_size,
        * cores: cores,

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
    let size = settings.get::<usize>("size");
    let cores = settings.get::<usize>("cores");
    let warmup = settings.get::<bool>("warmup");
    let calibrate = settings.get::<bool>("calibrated");
    let zswap_max_pool_percent = settings.get::<usize>("zswap_max_pool_percent");

    // Reboot
    initial_reboot(dry_run, &login)?;

    // Connect to host
    let mut ushell = connect_and_setup_host_only(dry_run, &login)?;

    // Turn on SSDSWAP.
    turn_on_ssdswap(&ushell, dry_run)?;

    // Start and connect to VM
    let vshell = start_vagrant(&ushell, &login.host, vm_size, cores)?;

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
        //const WARM_UP_SIZE: usize = 50; // GB
        const WARM_UP_PATTERN: &str = "-z";
        vshell.run(
            cmd!(
                "sudo ./target/release/time_mmap_touch {} {} > /dev/null",
                (size << 30) >> 12,
                WARM_UP_PATTERN,
            )
            .cwd(zerosim_exp_path)
            .use_bash(),
        )?;
    }

    vshell.run(
        cmd!(
            "taskset -c 0 ./bin/cg.E.x > {}/{}",
            VAGRANT_RESULTS_DIR,
            output_file
        )
        .cwd(&format!("{}/NPB3.4/NPB3.4-OMP", zerosim_bmk_path)),
    )?;

    ushell.run(cmd!("date"))?;

    Ok(())
}
