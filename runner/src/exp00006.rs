//! Boot the kernel and dump the contents of `/proc/ktask_instrumentation`.
//!
//! Requires `setup00000` followed by `setup00001` with the `markm_instrument_ktask` or
//! `markm_instrument_mem_init` kernel.

use clap::clap_app;

use spurs::{
    cmd,
    ssh::{Execute, SshShell},
    util::escape_for_bash,
};

use crate::{
    common::{exp_0sim::*, output::OutputManager, paths::setup00000::*},
    settings,
};

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
        (@group KTASK_DIV =>
            (@attributes +required)
            (@arg DIV: +takes_value {is_usize}
             "The scaling factor to pass a boot parameter. The max number of threads \
              in ktask is set to `CORES / KTASK_DIV`. 4 is the default for \
              normal ktask.")
            (@arg NO_KTASK: --no_ktask
             "Measure boot without ktask.")
        )
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
    let vm_size = sub_m.value_of("VMSIZE").unwrap().parse::<usize>().unwrap();
    let cores = sub_m.value_of("CORES").unwrap().parse::<usize>().unwrap();
    let ktask_div = sub_m.value_of("DIV").map(|s| s.parse::<usize>().unwrap());

    let ushell = SshShell::with_default_key(&login.username.as_str(), &login.host)?;
    let local_git_hash = crate::common::local_research_workspace_git_hash()?;
    let remote_git_hash = crate::common::research_workspace_git_hash(&ushell)?;
    let remote_research_settings = crate::common::get_remote_research_settings(&ushell)?;

    let settings = settings! {
        * workload: if ktask_div.is_some() { "ktask_boot_mem_init" } else { "boot_mem_init" },
        exp: 00006,

        * vm_size: vm_size,
        * cores: cores,

        (ktask_div.is_some()) ktask_div: ktask_div,

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
    A: std::net::ToSocketAddrs + std::fmt::Display + std::fmt::Debug,
{
    let vm_size = settings.get::<usize>("vm_size");
    let cores = settings.get::<usize>("cores");
    let ktask_div = settings.get::<Option<usize>>("ktask_div");

    // Collect timers on VM
    let mut timers = vec![];

    // We first need to set the guest kernel boot param.
    if let Some(ktask_div) = ktask_div {
        let ushell = SshShell::with_default_key(login.username.as_str(), login.hostname)?;
        let vshell = time!(
            timers,
            "Start VM (for boot param setting)",
            start_vagrant(
                &ushell,
                &login.host,
                /* RAM */ 10,
                /* cores */ 1,
                /* fast */ true
            )?
        );

        set_kernel_boot_param(
            &vshell,
            "ktask_mem_ncores_div",
            Some(&format!("{}", ktask_div)),
        )?;

        // Allow-error doesn't work because there will be a transport error, not a command failure.
        let _ = vshell.run(cmd!("sudo poweroff"));
    }

    // Reboot
    initial_reboot(dry_run, &login)?;

    // Connect
    let ushell = connect_and_setup_host_only(dry_run, &login)?;

    let vshell = time!(
        timers,
        "Start VM",
        start_vagrant(&ushell, &login.host, vm_size, cores, /* fast */ false)?
    );

    let (output_file, params_file) = settings.gen_file_names();
    let time_file = settings.gen_file_name("time");
    let params = serde_json::to_string(&settings)?;

    vshell.run(cmd!(
        "echo '{}' > {}",
        escape_for_bash(&params),
        dir!(VAGRANT_RESULTS_DIR, params_file)
    ))?;

    vshell.run(cmd!(
        "cat /proc/ktask_instrumentation > {}",
        dir!(VAGRANT_RESULTS_DIR, output_file)
    ))?;

    ushell.run(cmd!("date"))?;

    vshell.run(cmd!(
        "echo -e '{}' > {}",
        crate::common::timings_str(timers.as_slice()),
        dir!(VAGRANT_RESULTS_DIR, time_file)
    ))?;

    if print_results_path {
        let glob = settings.gen_file_name("*");
        println!("RESULTS: {}", glob);
    }

    Ok(())
}
