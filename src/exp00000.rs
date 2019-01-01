//! Run the time_mmap_touch or memcached_gen_data workload on the remote cloudlab machine.
//!
//! Requires `setup00000`.

use spurs::cmd;

use crate::common::exp00000::*;

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
                .cwd("/home/vagrant/paperexp")
                .use_bash(),
            )?;
        }

        // Then, run the actual experiment
        vshell.run(
            cmd!(
                "sudo ./target/release/time_mmap_touch {} {} \
                 > /vagrant/vm_shared/results/time_mmap_touch_{}gb_zero_zswap_ssdswap_vm{}gb{}_{}.out",
                (size << 30) >> 12,
                pattern,
                size,
                vm_size,
                if warmup { "_warmedup" } else { "" },
                chrono::offset::Local::now()
                    .format("%Y-%m-%d-%H-%M-%S")
                    .to_string()
            )
            .cwd("/home/vagrant/paperexp")
            .use_bash(),
        )?;
    } else {
        vshell.run(cmd!("memcached -M -m {} -d -u vagrant", (size * 1024)))?;

        // We allow errors because the memcached -M flag errors on OOM rather than doing an insert.
        // This gives much simpler performance behaviors. memcached uses a large amount of the memory
        // you give it for bookkeeping, rather than user data, so OOM will almost certainly happen.
        vshell.run(
            cmd!(
                "./target/release/memcached_gen_data localhost:11211 {} \
                 > /vagrant/vm_shared/results/memcached_{}gb_zswap_ssdswap_{}.out",
                size,
                size,
                chrono::offset::Local::now()
                    .format("%Y-%m-%d-%H-%M-%S")
                    .to_string()
            )
            .cwd("/home/vagrant/paperexp")
            .use_bash()
            .allow_error(),
        )?;
    }

    ushell.run(cmd!("date"))?;

    spurs::util::reboot(&mut ushell, dry_run)?;

    Ok(())
}
