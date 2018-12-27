//! Run a memcached workload on the remote cloudlab machine designed to induce THP compaction
//! remotely. Measure the number of per-page operations done and undone.
//!
//! Requires `setup00000` followed by `setup00001`.

use spurs::cmd;

use crate::common::exp00003::*;

/// Interval at which to collect thp stats
const INTERVAL: usize = 60; // seconds

pub fn run<A: std::net::ToSocketAddrs + std::fmt::Display>(
    dry_run: bool,
    login: &Login<A>,
    size: usize,    // GB
    vm_size: usize, // GB
    cores: Option<usize>,
) -> Result<(), failure::Error> {
    // Reboot
    initial_reboot(dry_run, &login)?;

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
    //vshell.run(cmd!("sudo ./target/release/time_calibrate").cwd("/home/vagrant/paperexp"))?;

    // Turn on compaction and force it too happen
    vshell.run(
        cmd!("echo always | sudo tee /sys/kernel/mm/transparent_hugepage/enabled").use_bash(),
    )?;
    vshell.run(
        cmd!("echo always | sudo tee /sys/kernel/mm/transparent_hugepage/defrag").use_bash(),
    )?;
    vshell.run(
        cmd!("echo 1 | sudo tee /sys/kernel/mm/transparent_hugepage/khugepaged/defrag").use_bash(),
    )?;
    vshell.run(
        cmd!("echo 1000 | sudo tee /sys/kernel/mm/transparent_hugepage/khugepaged/alloc_sleep_millisecs").use_bash(),
    )?;
    vshell.run(
        cmd!("echo 1000 | sudo tee /sys/kernel/mm/transparent_hugepage/khugepaged/scan_sleep_millisecs").use_bash(),
    )?;

    // Run memcached. We need to make it take slightly less memory than the VM, or it will OOM.
    vshell.run(cmd!(
        "memcached -m {} -d -u vagrant",
        (vm_size * 1024 * 95 / 100) // 95% of VM
    ))?;

    vshell.run(
        cmd!(
            "nohup ./target/release/memcached_and_capture_thp localhost:11211 {} {} \
             > /vagrant/vm_shared/results/memcached_and_capture_thp_{}gb_zswap_ssdswap_vm{}gb_{}.out",
            size,
            INTERVAL,
            size,
            vm_size,
            chrono::offset::Local::now()
                .format("%Y-%m-%d-%H-%M-%S")
                .to_string()
        )
        .cwd("/home/vagrant/paperexp")
        .use_bash()
        .allow_error(),
    )?;

    ushell.run(cmd!("date"))?;

    spurs::util::reboot(&mut ushell, dry_run)?;

    Ok(())
}
