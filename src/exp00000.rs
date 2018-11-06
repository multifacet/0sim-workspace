//! Run the time_mmap_touch or memcached_gen_data workload on the remote cloudlab machine.
//!
//! Requires `setup00000`.

use std::collections::HashMap;

use spurs::{cmd, ssh::SshShell};

/// The port that vagrant VMs forward from.
pub const VAGRANT_PORT: u16 = 5555;

/// The amount of memory of the VM.
pub const VAGRANT_MEM: usize = 1024;

pub fn run<A: std::net::ToSocketAddrs + std::fmt::Display>(
    dry_run: bool,
    cloudlab: A,
    username: &str,
    size: usize,
    pattern: Option<&str>,
) -> Result<(), failure::Error> {
    // Reboot
    initial_reboot(dry_run, &cloudlab, username)?;

    // Connect
    let (mut ushell, vshell) = connect_and_setup_host_and_vagrant(dry_run, &cloudlab, username)?;

    // Environment
    turn_on_zswap(&mut ushell, dry_run)?;

    ushell
        .run(cmd!("echo 50 | sudo tee /sys/module/zswap/parameters/max_pool_percent").use_bash())?;

    // Run memcached or time_touch_mmap
    if let Some(pattern) = pattern {
        // Warm up
        //const WARM_UP_SIZE: usize = 50; // GB
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

        // Then, run the actual experiment
        vshell.run(
            cmd!(
                "sudo ./target/release/time_mmap_touch {} {} \
                 > /vagrant/vm_shared/results/time_mmap_touch_{}gb_zero_zswap_ssdswap_{}.out",
                (size << 30) >> 12,
                pattern,
                size,
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
                "nohup ./target/release/memcached_gen_data localhost:11211 {} \
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

    spurs::util::reboot(&mut ushell, dry_run)?;

    Ok(())
}

pub fn run_setup_only<A: std::net::ToSocketAddrs + std::fmt::Display>(
    dry_run: bool,
    cloudlab: A,
    username: &str,
) -> Result<(), failure::Error> {
    // Connect
    let _ = connect_and_setup_host_and_vagrant(dry_run, &cloudlab, username)?;

    Ok(())
}

/// Reboot the machine and do nothing else. Useful for getting the machine into a clean state.
fn initial_reboot<A: std::net::ToSocketAddrs + std::fmt::Display>(
    dry_run: bool,
    cloudlab: A,
    username: &str,
) -> Result<(), failure::Error> {
    // Connect to the remote
    let mut ushell = SshShell::with_default_key(username, &cloudlab)?;
    if dry_run {
        ushell.toggle_dry_run();
    }

    // Reboot the remote to make sure we have a clean slate
    spurs::util::reboot(&mut ushell, dry_run)?;

    Ok(())
}

/// Connects to the host and to vagrant. Returns shells for both.
fn connect_and_setup_host_and_vagrant<A: std::net::ToSocketAddrs + std::fmt::Display>(
    dry_run: bool,
    cloudlab: A,
    username: &str,
) -> Result<(SshShell, SshShell), failure::Error> {
    let ushell = connect_and_setup_host_only(dry_run, &cloudlab, username)?;
    let vshell = start_vagrant(&ushell, &cloudlab, VAGRANT_MEM)?;

    Ok((ushell, vshell))
}

/// Connects to the host, waiting for it to come up if necessary. Turn on only the swap devices we
/// want. Set the scaling governor. Returns the shell to the host.
fn connect_and_setup_host_only<A: std::net::ToSocketAddrs + std::fmt::Display>(
    dry_run: bool,
    cloudlab: A,
    username: &str,
) -> Result<SshShell, failure::Error> {
    // Keep trying to connect until we succeed
    let ushell = {
        let mut shell;
        loop {
            shell = match SshShell::with_default_key(username, &cloudlab) {
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
    turn_off_swapdevs(&ushell, dry_run)?;
    turn_on_swapdevs(&ushell, dry_run)?;

    // Make sure /proj/superpages-PG0 is mounted
    let nfs_mounted = ushell.run(cmd!("mount | grep proj").use_bash()).is_ok();
    if !nfs_mounted {
        ushell.run(
            cmd!("sudo mount -t nfs -o rw,relatime,vers=3,rsize=131072,wsize=131072,namlen=255,hard,nolock,proto=tcp,timeo=600,ys,mountaddr=128.104.222.8,mountvers=3,mountport=900,mountproto=tcp,local_lock=all,addr=128.104.222.8 \
                  128.104.222.8:/proj/superpages-PG0 /proj/superpages-PG0/")
        )?;
    }

    println!("Assuming home dir already mounted... uncomment this line if it's not");
    //mount_home_dir(ushell)

    ushell.run(
        cmd!("make")
            .cwd("/users/markm/linux-dev/tools/power/cpupower/")
            .dry_run(dry_run),
    )?;
    ushell.run(cmd!("sudo /users/markm/linux-dev/tools/power/cpupower/cpupower frequency-set -g performance").dry_run(dry_run))?;

    ushell.run(cmd!("echo 4 | sudo tee /proc/sys/kernel/printk").use_bash())?;

    Ok(ushell)
}

/// Turn on Zswap with some default parameters.
fn turn_on_zswap(shell: &mut SshShell, dry_run: bool) -> Result<(), failure::Error> {
    if dry_run {
        shell.toggle_dry_run();
    }

    // apparently permissions can get weird
    shell.run(cmd!("sudo chmod +w /sys/module/zswap/parameters/*").use_bash())?;

    // THP is buggy with frontswap until later kernels
    shell.run(
        cmd!("echo never | sudo tee /sys/kernel/mm/transparent_hugepage/enabled").use_bash(),
    )?;

    // KSM is also not working right
    shell.run(cmd!("sudo systemctl disable ksm"))?;
    shell.run(cmd!("sudo systemctl disable ksmtuned"))?;

    shell.run(cmd!("echo ztier | sudo tee /sys/module/zswap/parameters/zpool").use_bash())?;
    shell.run(cmd!("echo y | sudo tee /sys/module/zswap/parameters/enabled").use_bash())?;
    shell.run(cmd!("sudo tail /sys/module/zswap/parameters/*").use_bash())?;

    if dry_run {
        shell.toggle_dry_run();
    }

    Ok(())
}

pub fn connect_to_vagrant<A: std::net::ToSocketAddrs + std::fmt::Display>(
    cloudlab: A,
) -> Result<SshShell, failure::Error> {
    let (host, _) = spurs::util::get_host_ip(cloudlab);
    SshShell::with_default_key("root", (host, VAGRANT_PORT))
}

pub fn start_vagrant<A: std::net::ToSocketAddrs + std::fmt::Display>(
    shell: &SshShell,
    cloudlab: A,
    memgb: usize,
) -> Result<SshShell, failure::Error> {
    shell.run(cmd!("sudo systemctl stop firewalld"))?;
    shell.run(cmd!("sudo systemctl stop nfs-idmap.service"))?;
    shell.run(cmd!("sudo systemctl start nfs-idmap.service"))?;

    gen_vagrantfile_gb(shell, memgb)?;

    shell.run(cmd!("vagrant halt").cwd("/proj/superpages-PG0/markm_vagrant/"))?;
    shell.run(
        cmd!("vagrant up")
            .no_pty()
            .cwd("/proj/superpages-PG0/markm_vagrant/"),
    )?;
    shell.run(cmd!("sudo lsof -i -P -n | grep LISTEN").use_bash())?;
    let vshell = connect_to_vagrant(cloudlab)?;

    // Pin vcpus
    let pin = {
        let mut pin = HashMap::new();
        pin.insert(0, 0);
        pin
    };
    virsh_vcpupin(shell, &pin)?;

    Ok(vshell)
}

fn turn_off_swapdevs(shell: &SshShell, dry_run: bool) -> Result<(), failure::Error> {
    let devs = spurs::util::get_mounted_devs(shell, dry_run)?;

    // Turn off all swap devs
    for (dev, mount) in devs {
        if mount == "[SWAP]" {
            shell.run(cmd!("sudo swapoff /dev/{}", dev))?;
        }
    }

    Ok(())
}

fn turn_on_swapdevs(shell: &SshShell, dry_run: bool) -> Result<(), failure::Error> {
    // Find out what swap devs are there
    let devs = spurs::util::get_unpartitioned_devs(shell, dry_run)?;

    // Get the size of each one
    let sizes = spurs::util::get_dev_sizes(shell, &devs, dry_run)?;

    // Turn on the SSDs as swap devs
    for (dev, size) in devs.iter().zip(sizes.iter()) {
        if size == "447.1G" {
            shell.run(cmd!("sudo swapon /dev/{}", dev))?;
        }
    }

    // Turn on the HDDs as swap devs
    for (dev, size) in devs.iter().zip(sizes.iter()) {
        if ["2.7T", "3.7T", "931.5G"].iter().any(|s| s == size) {
            shell.run(cmd!("sudo swapon /dev/{}", dev))?;
        }
    }

    shell.run(cmd!("lsblk"))?;

    Ok(())
}

/// For `(v, p)` in `mapping`, pin vcpu `v` to host cpu `p`.
fn virsh_vcpupin(shell: &SshShell, mapping: &HashMap<usize, usize>) -> Result<(), failure::Error> {
    shell.run(cmd!("sudo virsh vcpuinfo markm_vagrant_test_vm"))?;

    for (v, p) in mapping {
        shell.run(cmd!("sudo virsh vcpupin markm_vagrant_test_vm {} {}", v, p))?;
    }

    shell.run(cmd!("sudo virsh vcpuinfo markm_vagrant_test_vm"))?;

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
        .cwd("/proj/superpages-PG0/markm_vagrant/"),
    )?;

    Ok(())
}
