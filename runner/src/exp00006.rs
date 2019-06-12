//! Boot the kernel and dump the contents of `/proc/ktask_instrumentation`.
//!
//! Requires `setup00000` followed by `setup00001` with the `markm_instrument_ktask` kernel.

use clap::clap_app;

use spurs::{
    cmd,
    ssh::{Execute, SshShell},
    util::escape_for_bash,
};

use crate::common::{exp00003::*, output::OutputManager};
use crate::settings;

pub fn cli_options() -> clap::App<'static, 'static> {
    fn is_usize(s: String) -> Result<(), String> {
        s.as_str()
            .parse::<usize>()
            .map(|_| ())
            .map_err(|e| format!("{:?}", e))
    }

    clap_app! { exp00006 =>
        (about: "Run experiment 00006. Requires `sudo`.")
        (@arg HOSTNAME: +required +takes_value
         "The domain name of the remote (e.g. c240g2-031321.wisc.cloudlab.us:22)")
        (@arg USERNAME: +required +takes_value
         "The username on the remote (e.g. markm)")
        (@arg VMSIZE: +takes_value {is_usize} +required
         "The number of GBs of the VM")
        (@arg CORES: +takes_value {is_usize} +required
         "The number of cores of the VM")
    }
}

pub fn run(dry_run: bool, sub_m: &clap::ArgMatches<'_>) -> Result<(), failure::Error> {
    let login = Login {
        username: Username(sub_m.value_of("USERNAME").unwrap()),
        hostname: sub_m.value_of("HOSTNAME").unwrap(),
        host: sub_m.value_of("HOSTNAME").unwrap(),
    };
    let vm_size = sub_m.value_of("VMSIZE").unwrap().parse::<usize>().unwrap();
    let cores = sub_m.value_of("CORES").unwrap().parse::<usize>().unwrap();

    let ushell = SshShell::with_default_key(&login.username.as_str(), &login.host)?;
    let local_git_hash = crate::common::local_research_workspace_git_hash()?;
    let remote_git_hash = crate::common::research_workspace_git_hash(&ushell)?;
    let remote_research_settings = crate::common::get_remote_research_settings(&ushell)?;

    let settings = settings! {
        * workload: "ktask_boot_mem_init",
        exp: 00006,

        * vm_size: vm_size,
        * cores: cores,

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
    let cores = settings.get::<usize>("cores");

    // Reboot
    initial_reboot(dry_run, &login)?;

    // Collect timers on VM
    let mut timers = vec![];

    // Connect
    let ushell = connect_and_setup_host_only(dry_run, &login)?;

    let vshell = time!(
        timers,
        "Start VM",
        start_vagrant(&ushell, &login.host, vm_size, cores)?
    );

    let (output_file, params_file) = settings.gen_file_names();
    let time_file = settings.gen_file_name("time");
    let params = serde_json::to_string(&settings)?;

    vshell.run(cmd!(
        "echo '{}' > {}/{}",
        escape_for_bash(&params),
        VAGRANT_RESULTS_DIR,
        params_file
    ))?;

    vshell.run(cmd!(
        "cat /proc/ktask_instrumentation > {}/{}",
        VAGRANT_RESULTS_DIR,
        output_file
    ))?;

    ushell.run(cmd!("date"))?;

    vshell.run(cmd!(
        "echo -e '{}' > {}/{}",
        crate::common::timings_str(timers.as_slice()),
        VAGRANT_RESULTS_DIR,
        time_file
    ))?;

    Ok(())
}
