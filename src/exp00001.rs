//! Run the time_mmap_touch workload on the remote desktop machine.
//!
//! Requires the desktop machine to already be set up, similarly to `setup00000`, with a minor
//! exception: we run everything as root.

use std::collections::HashMap;

use spurs::{cmd, ssh::SshShell};

/// The port that vagrant VMs forward from.
pub const VAGRANT_PORT: u16 = 5555;

/// The amount of memory of the VM.
pub const VAGRANT_MEM: usize = 1023;

pub fn run<A: std::net::ToSocketAddrs + std::fmt::Display>(
    dry_run: bool,
    desktop: A,
    username: &str,
    size: usize,
    pattern: &str,
) -> Result<(), failure::Error> {
    // Reboot
    initial_reboot(dry_run, &desktop)?;

    // Connect
    let (_ushell, mut rshell, vshell) =
        connect_and_setup_host_and_vagrant(dry_run, username, &desktop)?;

    // Environment
    turn_on_zswap(&mut rshell, dry_run)?;

    rshell.run(cmd!("echo 50 > /sys/module/zswap/parameters/max_pool_percent").use_bash())?;

    // Warm up
    //const WARM_UP_SIZE: usize = 50; // GB
    const WARM_UP_PATTERN: &str = "-z";
    vshell.run(
        cmd!(
            "./target/release/time_mmap_touch {} {} > /dev/null",
            //(WARM_UP_SIZE << 30) >> 12,
            //WARM_UP_PATTERN,
            (size << 30) >> 12,
            WARM_UP_PATTERN,
        )
        .cwd("/home/vagrant/paperexp")
        .use_bash(),
    )?;

    // Then, run the actual experiment
    vshell.run(
        cmd!("./target/release/time_mmap_touch {} {} > /vagrant/vm_shared/results/time_mmap_touch_{}gb_zero_zswap_ssdswap_{}.out",
             (size << 30) >> 12,
             pattern,
             size,
             chrono::offset::Local::now().format("%Y-%m-%d-%H-%M-%S").to_string()
        )
        .cwd("/home/vagrant/paperexp")
        .use_bash()
    )?;

    reboot(&mut rshell, dry_run)?;

    Ok(())
}

pub fn reboot(shell: &mut SshShell, dry_run: bool) -> Result<(), failure::Error> {
    let _ = shell.run(cmd!("reboot").dry_run(dry_run));

    if !dry_run {
        // If we try to reconnect immediately, the machine will not have gone down yet.
        std::thread::sleep(std::time::Duration::from_secs(10));

        // Attempt to reconnect.
        shell.reconnect()?;
    }

    // Make sure it worked.
    shell.run(cmd!("whoami").dry_run(dry_run))?;

    Ok(())
}

/// Reboot the machine and do nothing else. Useful for getting the machine into a clean state.
fn initial_reboot<A: std::net::ToSocketAddrs + std::fmt::Display>(
    dry_run: bool,
    desktop: A,
) -> Result<(), failure::Error> {
    // Connect to the remote
    let mut ushell = SshShell::with_default_key("root", &desktop)?;
    if dry_run {
        ushell.toggle_dry_run();
    }

    // Reboot the remote to make sure we have a clean slate
    reboot(&mut ushell, dry_run)?;

    Ok(())
}

/// Connects to the host and to vagrant. Returns shells for both.
fn connect_and_setup_host_and_vagrant<A: std::net::ToSocketAddrs + std::fmt::Display>(
    dry_run: bool,
    username: &str,
    desktop: A,
) -> Result<(SshShell, SshShell, SshShell), failure::Error> {
    let (ushell, rshell) = connect_and_setup_host_only(dry_run, username, &desktop)?;
    let vshell = start_vagrant(&ushell, &desktop, VAGRANT_MEM)?;

    Ok((ushell, rshell, vshell))
}

/// Connects to the host, waiting for it to come up if necessary. Turn on only the swap devices we
/// want. Set the scaling governor. Returns the shell to the host.
fn connect_and_setup_host_only<A: std::net::ToSocketAddrs + std::fmt::Display>(
    dry_run: bool,
    username: &str,
    desktop: A,
) -> Result<(SshShell, SshShell), failure::Error> {
    // Keep trying to connect until we succeed
    let rshell = {
        let mut shell;
        loop {
            shell = match SshShell::with_default_key("root", &desktop) {
                Ok(shell) => shell,
                Err(_) => {
                    std::thread::sleep(std::time::Duration::from_secs(10));
                    continue;
                }
            };
            match shell.run(cmd!("whoami")) {
                Ok(_) => break,
                Err(_) => {
                    std::thread::sleep(std::time::Duration::from_secs(10));
                    continue;
                }
            }
        }

        shell
    };

    let ushell = {
        let mut shell;
        loop {
            shell = match SshShell::with_default_key(username, &desktop) {
                Ok(shell) => shell,
                Err(_) => {
                    std::thread::sleep(std::time::Duration::from_secs(10));
                    continue;
                }
            };
            match shell.run(cmd!("whoami")) {
                Ok(_) => break,
                Err(_) => {
                    std::thread::sleep(std::time::Duration::from_secs(10));
                    continue;
                }
            }
        }

        shell
    };

    ushell.run(cmd!("uname -a").dry_run(dry_run))?;

    // Set up swapping
    rshell.run(cmd!("swapoff /dev/sda5"))?;
    rshell.run(cmd!("swapon /dev/sdb"))?;

    ushell.run(
        cmd!("make")
            .cwd("/home/markm/linux-dev/tools/power/cpupower/")
            .dry_run(dry_run),
    )?;
    rshell.run(
        cmd!("/home/markm/linux-dev/tools/power/cpupower/cpupower frequency-set -g performance")
            .dry_run(dry_run),
    )?;

    rshell.run(cmd!("echo 4 > /proc/sys/kernel/printk").use_bash())?;

    Ok((ushell, rshell))
}

/// Turn on Zswap with some default parameters.
fn turn_on_zswap(shell: &mut SshShell, dry_run: bool) -> Result<(), failure::Error> {
    if dry_run {
        shell.toggle_dry_run();
    }

    // apparently permissions can get weird
    shell.run(cmd!("chmod +w /sys/module/zswap/parameters/*").use_bash())?;

    // THP is buggy with frontswap until later kernels
    shell.run(cmd!("echo never > /sys/kernel/mm/transparent_hugepage/enabled").use_bash())?;

    shell.run(cmd!("echo ztier > /sys/module/zswap/parameters/zpool").use_bash())?;
    shell.run(cmd!("echo y > /sys/module/zswap/parameters/enabled").use_bash())?;
    shell.run(cmd!("tail /sys/module/zswap/parameters/*").use_bash())?;

    if dry_run {
        shell.toggle_dry_run();
    }

    Ok(())
}

pub fn connect_to_vagrant<A: std::net::ToSocketAddrs + std::fmt::Display>(
    desktop: A,
) -> Result<SshShell, failure::Error> {
    let (host, _) = spurs::util::get_host_ip(desktop);
    SshShell::with_default_key("root", (host, VAGRANT_PORT))
}

pub fn start_vagrant<A: std::net::ToSocketAddrs + std::fmt::Display>(
    shell: &SshShell,
    desktop: A,
    memgb: usize,
) -> Result<SshShell, failure::Error> {
    gen_vagrantfile_gb(shell, memgb)?;

    shell.run(cmd!("vagrant halt").cwd("/home/markm/vagrant/"))?;
    shell.run(cmd!("vagrant up").no_pty().cwd("/home/markm/vagrant/"))?;
    shell.run(cmd!("lsof -i -P -n | grep LISTEN").use_bash())?;
    let vshell = connect_to_vagrant(desktop)?;

    // Pin vcpus
    let pin = {
        let mut pin = HashMap::new();
        pin.insert(0, 0);
        pin
    };
    virsh_vcpupin(shell, &pin)?;

    Ok(vshell)
}

/// For `(v, p)` in `mapping`, pin vcpu `v` to host cpu `p`.
fn virsh_vcpupin(shell: &SshShell, mapping: &HashMap<usize, usize>) -> Result<(), failure::Error> {
    shell.run(cmd!("virsh vcpuinfo vagrant_test_vm"))?;

    for (v, p) in mapping {
        shell.run(cmd!("virsh vcpupin vagrant_test_vm {} {}", v, p))?;
    }

    shell.run(cmd!("virsh vcpuinfo vagrant_test_vm"))?;

    Ok(())
}

/// Generate a Vagrantfile for a VM with the given amount of memory.
pub fn gen_vagrantfile_gb(shell: &SshShell, memgb: usize) -> Result<(), failure::Error> {
    shell.run(
        cmd!(
            "sed 's/memory = 1023/memory = {}/' Vagrantfile.bk > Vagrantfile",
            memgb
        )
        .use_bash()
        .cwd("/home/markm/vagrant/"),
    )?;

    Ok(())
}
