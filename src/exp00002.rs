//! Run the time_loop workload on the remote cloudlab machine.
//!
//! Requires `setup00000`.

use spurs::cmd;

use crate::common::exp00002::*;

pub fn run<A>(
    dry_run: bool,
    login: &Login<A>,
    n: usize,               // GB
    vm_size: Option<usize>, // GB
    cores: Option<usize>,
    warmup: bool,
) -> Result<(), failure::Error>
where
    A: std::net::ToSocketAddrs + std::fmt::Display + std::fmt::Debug,
{
    // Reboot
    initial_reboot(dry_run, &login)?;

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

    // Connect
    let (mut ushell, vshell) = connect_and_setup_host_and_vagrant(dry_run, &login, vm_size, cores)?;

    // Environment
    turn_on_zswap(&mut ushell, dry_run)?;

    ushell
        .run(cmd!("echo 50 | sudo tee /sys/module/zswap/parameters/max_pool_percent").use_bash())?;

    // Calibrate
    vshell.run(cmd!("sudo ./target/release/time_calibrate").cwd("/home/vagrant/paperexp"))?;

    // Warm up
    if warmup {
        const WARM_UP_PATTERN: &str = "-z";
        vshell.run(
            cmd!(
                "sudo ./target/release/time_mmap_touch {} {} > /dev/null",
                ((vm_size << 30) >> 12) >> 1,
                WARM_UP_PATTERN,
            )
            .cwd("/home/vagrant/paperexp")
            .use_bash(),
        )?;
    }

    // Then, run the actual experiment
    vshell.run(
            cmd!(
                "sudo ./target/release/time_loop {} > /vagrant/vm_shared/results/time_loop_{}_zswap_ssdswap_vm{}gb{}_{}.out",
                n,
                n,
                vm_size,
                if warmup { "_warmedup" } else { "" },
                chrono::offset::Local::now()
                    .format("%Y-%m-%d-%H-%M-%S")
                    .to_string()
            )
            .cwd("/home/vagrant/paperexp")
            .use_bash(),
        )?;

    ushell.run(cmd!("date"))?;

    spurs::util::reboot(&mut ushell, dry_run)?;

    Ok(())
}
