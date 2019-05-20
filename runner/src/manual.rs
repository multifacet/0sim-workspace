//! Runner for common routines like setting up an environment to manually run experiments. Note
//! that this is not setup as in "take a stock VM and install stuff" but rather "take a machine
//! with stuff installed and prepare the environment for an experiment (e.g. setting scaling
//! governor)".
//!
//! NOTE: This should not be used for real experiments. Just for testing and prototyping.

use clap::ArgMatches;

use spurs::{
    cmd,
    ssh::{Execute, SshShell},
};

use crate::common::{
    exp00000::{
        initial_reboot, set_kernel_printk_level, set_perf_scaling_gov, setup_swapping,
        start_vagrant, turn_on_ssdswap, turn_on_zswap, VAGRANT_CORES, VAGRANT_MEM,
    },
    Login, Username,
};

pub fn run(dry_run: bool, sub_m: &ArgMatches<'_>) -> Result<(), failure::Error> {
    // Read all flags/options
    let login = Login {
        username: Username(sub_m.value_of("USERNAME").unwrap()),
        hostname: sub_m.value_of("HOSTNAME").unwrap(),
        host: sub_m.value_of("HOSTNAME").unwrap(),
    };
    let reboot = sub_m.is_present("REBOOT");
    let swap = sub_m.is_present("SWAP");
    let perfgov = sub_m.is_present("PERF");
    let printk = sub_m
        .value_of("PRINTK")
        .map(|value| value.parse::<usize>().unwrap());
    let ssdswap = sub_m.is_present("SSDSWAP");
    let vm = sub_m.is_present("VM");
    let vm_size = sub_m
        .value_of("VMSIZE")
        .map(|value| value.parse::<usize>().unwrap());
    let vm_cores = sub_m
        .value_of("VMCORES")
        .map(|value| value.parse::<usize>().unwrap());
    let zswap = sub_m
        .value_of("ZSWAP")
        .map(|value| value.parse::<usize>().unwrap());

    // Reboot
    if reboot {
        initial_reboot(dry_run, &login)?;
    }

    let mut ushell = SshShell::with_default_key(&login.username.as_str(), &login.host)?;

    // Set up swap
    if swap {
        setup_swapping(&ushell, dry_run)?;
    }

    // Set scaling governor to "performance"
    if perfgov {
        set_perf_scaling_gov(&ushell, dry_run)?;
    }

    // Set printk level for dmesg
    if let Some(level) = printk {
        set_kernel_printk_level(&ushell, level)?;
    }

    // Turn on SSDSWAP
    if ssdswap {
        turn_on_ssdswap(&ushell, dry_run)?;
    }

    // Boot VM
    if vm {
        let vm_size = if let Some(vm_size) = vm_size {
            vm_size
        } else {
            VAGRANT_MEM
        };

        let vm_cores = if let Some(vm_cores) = vm_cores {
            vm_cores
        } else {
            VAGRANT_CORES
        };

        // Start and connect to VM
        let _ = start_vagrant(&ushell, &login.host, vm_size, vm_cores)?;
    }

    // Turn on zswap
    if let Some(max_pool_percent) = zswap {
        turn_on_zswap(&mut ushell, dry_run)?;

        ushell.run(
            cmd!(
                "echo {} | sudo tee /sys/module/zswap/parameters/max_pool_percent",
                max_pool_percent
            )
            .use_bash(),
        )?;
    }

    Ok(())
}
