//! Runner for common routines like setting up an environment to manually run experiments. Note
//! that this is not setup as in "take a stock VM and install stuff" but rather "take a machine
//! with stuff installed and prepare the environment for an experiment (e.g. setting scaling
//! governor)".
//!
//! NOTE: This should not be used for real experiments. Just for testing and prototyping.

use clap::{clap_app, ArgMatches};

use spurs::{cmd, Execute, SshShell};

use crate::common::{
    exp_0sim::{
        initial_reboot, set_kernel_printk_level, set_perf_scaling_gov, setup_swapping,
        start_vagrant, turn_on_ssdswap, ZeroSim, VAGRANT_CORES, VAGRANT_MEM, ZEROSIM_LAPIC_ADJUST,
        ZEROSIM_SKIP_HALT,
    },
    paths::*,
    Login,
};

pub fn cli_options() -> clap::App<'static, 'static> {
    fn is_usize(s: String) -> Result<(), String> {
        s.as_str()
            .parse::<usize>()
            .map(|_| ())
            .map_err(|e| format!("{:?}", e))
    }

    clap_app! { manual =>
        (about: "Perform some (non-strict) subset of the setup for an experiment. Requires `sudo`.")
        (@arg HOSTNAME: +required +takes_value
         "The domain name of the remote (e.g. c240g2-031321.wisc.cloudlab.us:22)")
        (@arg USERNAME: +required +takes_value
         "The username on the remote (e.g. markm)")
        (@arg REBOOT: --reboot
         "(Optional) If present, reboots the host machine.")
        (@arg SWAP: --setup_swap
         "(Optional) If present, setup swapping")
        (@arg PERF: --perfgov
         "(Optional) If present, set the scaling governor to \"performance\"")
        (@arg PRINTK: --printk +takes_value {is_usize}
         "(Optional) If present, set the printk logging level for dmesg. \
          0 = high-priority only. 7 = everything.")
        (@arg SSDSWAP: --ssdswap
         "(Optional) If present, turn on ssdswap.")
        (@arg VM: --vm
         "(Optional) Start the vagrant VM. Use other flags to set VM memory and vCPUS.")
        (@arg VMSIZE: --vm_size +takes_value {is_usize}
         "(Only valid with --vm) The number of GBs of the VM (defaults to 1024) (e.g. 500)")
        (@arg VMCORES: --vm_cores +takes_value {is_usize}
         "(Only valid with --vm) The number of cores of the VM (defaults to 1)")
        (@arg DISABLETSC: --disable_tsc
         "(Only valid with --vm) Disable TSC offsetting during boot to speed it up.")
        (@arg ZSWAP: --zswap +takes_value {is_usize}
         "(Optional) Turn on zswap with the given `max_pool_percent`")
        (@arg DRIFT_THRESHOLD: --drift_thresh +takes_value {is_usize}
         "(Optional) Set multicore offsetting drift threshold.")
        (@arg DELAY: --delay +takes_value {is_usize}
         "(Optional) Set multicore offsetting delay.")
        (@arg DISABLE_EPT: --disable_ept
         "(Optional) may need to disable Intel EPT on machines that don't have enough physical bits.")
        (@arg UPDATE_EXP: --update_exp
         "(Optional) if present, git pull 0sim-experiments and rebuild.")
    }
}

pub fn run(sub_m: &ArgMatches<'_>) -> Result<(), failure::Error> {
    // Read all flags/options
    let login = Login {
        username: sub_m.value_of("USERNAME").unwrap(),
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
    let disable_tsc = sub_m.is_present("DISABLETSC");
    let zswap = sub_m
        .value_of("ZSWAP")
        .map(|value| value.parse::<usize>().unwrap());
    let zerosim_drift_threshold = sub_m
        .value_of("DRIFT_THRESHOLD")
        .map(|value| value.parse::<usize>().unwrap());
    let zerosim_delay = sub_m
        .value_of("DELAY")
        .map(|value| value.parse::<usize>().unwrap());
    let disable_ept = sub_m.is_present("DISABLE_EPT");
    let update_exp = sub_m.is_present("UPDATE_EXP");

    // Reboot
    if reboot {
        initial_reboot(&login)?;
    }

    let mut ushell = SshShell::with_default_key(login.username, login.host)?;

    let user_home = crate::common::get_user_home_dir(&ushell)?;
    let zerosim_exp_path_host = &format!(
        "{}/{}/{}",
        user_home, RESEARCH_WORKSPACE_PATH, ZEROSIM_EXPERIMENTS_SUBMODULE
    );

    // Set up swap
    if swap {
        setup_swapping(&ushell)?;
    }

    // Set scaling governor to "performance"
    if perfgov {
        set_perf_scaling_gov(&ushell)?;
    }

    // Set printk level for dmesg
    if let Some(level) = printk {
        set_kernel_printk_level(&ushell, level)?;
    }

    // Turn on SSDSWAP
    if ssdswap {
        turn_on_ssdswap(&ushell)?;
    }

    // disable Intel EPT if needed
    if disable_ept {
        ushell.run(
            cmd!(r#"echo "options kvm-intel ept=0" | sudo tee /etc/modprobe.d/kvm-intel.conf"#)
                .use_bash(),
        )?;

        ushell.run(cmd!("sudo rmmod kvm_intel"))?;
        ushell.run(cmd!("sudo modprobe kvm_intel"))?;

        ushell.run(cmd!("sudo tail /sys/module/kvm_intel/parameters/ept"))?;
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
        let _ = start_vagrant(
            &ushell,
            &login.host,
            vm_size,
            vm_cores,
            disable_tsc,
            ZEROSIM_SKIP_HALT,
            ZEROSIM_LAPIC_ADJUST,
        )?;
    }

    // Turn on zswap
    if let Some(max_pool_percent) = zswap {
        ZeroSim::turn_on_zswap(&mut ushell)?;
        ZeroSim::zswap_max_pool_percent(&ushell, max_pool_percent)?;
    }

    // Set D and delta
    if let Some(zerosim_drift_threshold) = zerosim_drift_threshold {
        ZeroSim::threshold(&ushell, zerosim_drift_threshold)?;
    }
    if let Some(zerosim_delay) = zerosim_delay {
        ZeroSim::delay(&ushell, zerosim_delay)?;
    }

    // Update 0sim-experiments
    if update_exp {
        ushell.run(cmd!("git checkout master").cwd(zerosim_exp_path_host))?;
        ushell.run(cmd!("git pull").cwd(zerosim_exp_path_host))?;
        ushell.run(cmd!("~/.cargo/bin/cargo build --release").cwd(zerosim_exp_path_host))?;
    }

    Ok(())
}
