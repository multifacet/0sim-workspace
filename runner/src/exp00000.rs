//! Run the time_mmap_touch or memcached_gen_data workload on the remote cloudlab machine.
//!
//! Requires `setup00000`.

use spurs::{
    cmd,
    ssh::{Execute, SshShell},
    util::escape_for_bash,
};

use crate::common::{
    exp00000::*, output::OutputManager, RESEARCH_WORKSPACE_PATH, ZEROSIM_EXPERIMENTS_SUBMODULE,
};
use crate::settings;

pub fn run<A>(
    dry_run: bool,
    login: &Login<A>,
    size: usize, // GB
    pattern: Option<&str>,
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
    let git_hash = crate::common::research_workspace_git_hash(&ushell)?;

    let settings = settings! {
        git_hash: git_hash,
        exp: 00000,

        workload: if pattern.is_some() { "time_mmap_touch" } else { "memcached_gen_data" },
        * size: size,
        pattern: pattern,
        calibrated: false,
        warmup: warmup,

        * vm_size: vm_size,
        cores: cores,

        zswap_max_pool_percent: 50,

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
    let vm_size = settings.get::<usize>("vm_size");
    let size = settings.get::<usize>("size");
    let cores = settings.get::<usize>("cores");
    let pattern = settings.get::<Option<&str>>("pattern");
    let warmup = settings.get::<bool>("warmup");
    let calibrate = settings.get::<bool>("calibrated");
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
        "echo {} > {}/{}",
        escape_for_bash(&params),
        VAGRANT_RESULTS_DIR,
        params_file
    ))?;

    // Run memcached or time_touch_mmap
    if let Some(pattern) = pattern {
        // Warm up
        //const WARM_UP_SIZE: usize = 50; // GB
        if warmup {
            const WARM_UP_PATTERN: &str = "-z";
            vshell.run(
                cmd!(
                    "sudo ./target/release/time_mmap_touch {} {} > /dev/null",
                    //(WARM_UP_SIZE << 30) >> 12,
                    //WARM_UP_PATTERN,
                    (size << 30) >> 12,
                    WARM_UP_PATTERN,
                )
                .cwd(zerosim_exp_path)
                .use_bash(),
            )?;
        }

        // Then, run the actual experiment
        vshell.run(
            cmd!(
                "sudo ./target/release/time_mmap_touch {} {} > {}/{}",
                (size << 30) >> 12,
                pattern,
                VAGRANT_RESULTS_DIR,
                output_file,
            )
            .cwd(zerosim_exp_path)
            .use_bash(),
        )?;
    } else {
        vshell.run(cmd!("memcached -M -m {} -d -u vagrant", (size * 1024)))?;

        // We allow errors because the memcached -M flag errors on OOM rather than doing an insert.
        // This gives much simpler performance behaviors. memcached uses a large amount of the memory
        // you give it for bookkeeping, rather than user data, so OOM will almost certainly happen.
        vshell.run(
            cmd!(
                "./target/release/memcached_gen_data localhost:11211 {} > {}/{}",
                size,
                VAGRANT_RESULTS_DIR,
                output_file,
            )
            .cwd(zerosim_exp_path)
            .use_bash()
            .allow_error(),
        )?;
    }

    ushell.run(cmd!("date"))?;

    Ok(())
}
