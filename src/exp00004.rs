//! Run a memcached workload on the remote cloudlab machine designed to induce THP compaction
//! remotely. Measure the number of per-page operations done and undone. Unlike exp00003, run
//! this on the bare-metal host, rather than in a VM.
//!
//! Requires `setup00000` and `setup00002`.

use spurs::cmd;

use crate::common::exp00004::*;

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
    // Reboot
    initial_reboot(dry_run, &login)?;

    // Connect
    let ushell = connect_and_setup_host_only(dry_run, &login)?;

    ushell.run(cmd!("sudo swapon /dev/sda3"))?;

    // Turn on compaction and force it too happen
    ushell.run(
        cmd!("echo always | sudo tee /sys/kernel/mm/transparent_hugepage/enabled").use_bash(),
    )?;
    ushell.run(
        cmd!("echo always | sudo tee /sys/kernel/mm/transparent_hugepage/defrag").use_bash(),
    )?;
    ushell.run(
        cmd!("echo 1 | sudo tee /sys/kernel/mm/transparent_hugepage/khugepaged/defrag").use_bash(),
    )?;
    ushell.run(
        cmd!("echo 1000 | sudo tee /sys/kernel/mm/transparent_hugepage/khugepaged/alloc_sleep_millisecs").use_bash(),
    )?;
    ushell.run(
        cmd!("echo 1000 | sudo tee /sys/kernel/mm/transparent_hugepage/khugepaged/scan_sleep_millisecs").use_bash(),
    )?;

    // Run memcached. We need to make it take slightly less memory than RAM + swap, or it will OOM.
    ushell.run(cmd!("memcached -m {} -d", size * 1024))?;

    ushell.run(
        cmd!(
            "./target/release/memcached_and_capture_thp localhost:11211 {} {} \
             > ../vm_shared/results/memcached_and_capture_thp_{}gb_bare_metal_{}.out",
            size,
            INTERVAL,
            size,
            chrono::offset::Local::now()
                .format("%Y-%m-%d-%H-%M-%S")
                .to_string()
        )
        .cwd(&format!("/users/{}/paperexp", login.username.as_str()))
        .use_bash()
        .allow_error(),
    )?;

    ushell.run(cmd!("date"))?;

    ushell.run(cmd!("free -h"))?;

    Ok(())
}
