//! Routines used for 0sim-related experiments

use std::collections::HashMap;

use spurs::{cmd, Execute, SshError, SshShell};

use super::paths::*;

pub use super::Login;

/// The port that vagrant VMs forward from.
pub const VAGRANT_PORT: u16 = 5555;

/// The default amount of memory of the VM.
pub const VAGRANT_MEM: usize = 1024;

/// The default number of cores of the VM.
pub const VAGRANT_CORES: usize = 1;

/// The default value for /proc/zerosim_skip_halt.
///
/// Turning this on breaks the x86 ISA contract. Don't do that.
pub const ZEROSIM_SKIP_HALT: bool = false;

/// The default value for /proc/zerosim_lapic_adjust.
pub const ZEROSIM_LAPIC_ADJUST: bool = true;

/// Shut off any virtual machine and reboot the machine and do nothing else. Useful for getting the
/// machine into a clean state.
pub fn initial_reboot<A>(login: &Login<A>) -> Result<(), failure::Error>
where
    A: std::net::ToSocketAddrs + std::fmt::Display + std::fmt::Debug + Clone,
{
    // Connect to the remote
    let mut ushell = SshShell::with_default_key(login.username, &login.host)?;

    vagrant_halt(&ushell)?;

    // Reboot the remote to make sure we have a clean slate
    spurs_util::reboot(&mut ushell, /* dry_run */ false)?;

    Ok(())
}

/// Reboot the machine and do nothing else. Useful for getting the machine into a clean state. This
/// also attempts to turn off any virtual machines, but if there is an error, we ignore it and
/// reboot the host anyway.
pub fn initial_reboot_no_vagrant<A>(login: &Login<A>) -> Result<(), failure::Error>
where
    A: std::net::ToSocketAddrs + std::fmt::Display + std::fmt::Debug + Clone,
{
    // Connect to the remote
    let mut ushell = SshShell::with_default_key(login.username, &login.host)?;

    let _ = vagrant_halt(&ushell);

    // Reboot the remote to make sure we have a clean slate
    spurs_util::reboot(&mut ushell, /* dry_run */ false)?;

    Ok(())
}

/// Dump a bunch of kernel info for debugging.
pub fn dump_sys_info(shell: &SshShell) -> Result<(), failure::Error> {
    with_shell! { shell =>
        cmd!("uname -a"),
        cmd!("lsblk"),
        cmd!("free -h"),
    }

    Ok(())
}

/// Connects to the host and to vagrant. Returns shells for both. TSC offsetting is disabled
/// during VM startup to speed things up.
pub fn connect_and_setup_host_and_vagrant<A>(
    login: &Login<A>,
    vm_size: usize,
    cores: usize,
    skip_halt: bool,
    lapic_adjust: bool,
) -> Result<(SshShell, SshShell), failure::Error>
where
    A: std::net::ToSocketAddrs + std::fmt::Display + std::fmt::Debug + Clone,
{
    let ushell = connect_and_setup_host_only(&login)?;
    let vshell = start_vagrant(
        &ushell,
        &login.host,
        vm_size,
        cores,
        /* fast */ true,
        skip_halt,
        lapic_adjust,
    )?;

    Ok((ushell, vshell))
}

/// Turn off all previous swap spaces, and turn on the configured ones (e.g. via
/// research-settings.json).
pub fn setup_swapping(shell: &SshShell) -> Result<(), failure::Error> {
    turn_off_swapdevs(shell)?;
    turn_on_swapdevs(shell)?;
    Ok(())
}

/// Set the scaling governor to "performance".
pub fn set_perf_scaling_gov(shell: &SshShell) -> Result<(), failure::Error> {
    let user_home = crate::common::get_user_home_dir(shell)?;

    let kernel_path = format!(
        "{}/{}/{}",
        user_home, RESEARCH_WORKSPACE_PATH, ZEROSIM_KERNEL_SUBMODULE
    );

    shell.run(cmd!(
        "sudo {}/tools/power/cpupower/cpupower frequency-set -g performance",
        kernel_path
    ))?;

    Ok(())
}

/// Set the kernel `printk` level that gets logged to `dmesg`. `0` is only high-priority
/// messages. `7` is all messages.
pub fn set_kernel_printk_level(shell: &SshShell, level: usize) -> Result<(), failure::Error> {
    assert!(level <= 7);
    shell.run(cmd!("echo {} | sudo tee /proc/sys/kernel/printk", level).use_bash())?;
    Ok(())
}

/// Connects to the host, waiting for it to come up if necessary. Turn on only the swap devices we
/// want. Set the scaling governor. Returns the shell to the host.
pub fn connect_and_setup_host_only<A>(login: &Login<A>) -> Result<SshShell, failure::Error>
where
    A: std::net::ToSocketAddrs + std::fmt::Debug + std::fmt::Display + Clone,
{
    // Keep trying to connect until we succeed
    let ushell = {
        let mut shell;
        loop {
            shell = match SshShell::with_default_key(login.username, &login.host) {
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

    dump_sys_info(&ushell)?;

    // Force the VM off if it was left running. If there is no VM, then ignore errors.
    let _ = vagrant_halt(&ushell);

    // Set up swapping
    setup_swapping(&ushell)?;

    println!("Assuming home dir already mounted... uncomment this line if it's not");
    //mount_home_dir(ushell)

    set_perf_scaling_gov(&ushell)?;

    set_kernel_printk_level(&ushell, 4)?;

    Ok(ushell)
}

/// Set D for 0sim.
pub fn set_zerosim_threshold(shell: &SshShell, d: usize) -> Result<(), failure::Error> {
    shell.run(cmd!("echo {} | sudo tee /proc/zerosim_drift_threshold", d))?;
    Ok(())
}

pub fn set_zerosim_delay(shell: &SshShell, delay: usize) -> Result<(), failure::Error> {
    shell.run(cmd!("echo {} | sudo tee /proc/zerosim_delay", delay))?;
    Ok(())
}

/// Turn on Zswap with some default parameters.
pub fn turn_on_zswap(shell: &mut SshShell) -> Result<(), failure::Error> {
    // apparently permissions can get weird
    shell.run(cmd!("sudo chmod +w /sys/module/zswap/parameters/*").use_bash())?;

    // THP is buggy with frontswap until later kernels
    shell.run(
        cmd!("echo never | sudo tee /sys/kernel/mm/transparent_hugepage/enabled").use_bash(),
    )?;

    // KSM is also not working right
    if crate::common::service_is_running(shell, "ksm")? {
        shell.run(cmd!("sudo systemctl disable ksm"))?;
    }
    if crate::common::service_is_running(shell, "ksmtuned")? {
        shell.run(cmd!("sudo systemctl disable ksmtuned"))?;
    }

    shell.run(cmd!("echo ztier | sudo tee /sys/module/zswap/parameters/zpool").use_bash())?;
    shell.run(cmd!("echo y | sudo tee /sys/module/zswap/parameters/enabled").use_bash())?;
    shell.run(cmd!("sudo tail /sys/module/zswap/parameters/*").use_bash())?;

    Ok(())
}

pub fn connect_to_vagrant_user<A: std::net::ToSocketAddrs + std::fmt::Display>(
    hostname: A,
    user: &str,
) -> Result<SshShell, SshError> {
    let (host, _) = spurs_util::get_host_ip(hostname);
    SshShell::with_default_key(user, (host, VAGRANT_PORT))
}

pub fn connect_to_vagrant_as_root<A: std::net::ToSocketAddrs + std::fmt::Display>(
    hostname: A,
) -> Result<SshShell, SshError> {
    connect_to_vagrant_user(hostname, "root")
}

pub fn connect_to_vagrant_as_user<A: std::net::ToSocketAddrs + std::fmt::Display>(
    hostname: A,
) -> Result<SshShell, SshError> {
    connect_to_vagrant_user(hostname, "vagrant")
}

pub fn vagrant_halt(shell: &SshShell) -> Result<(), failure::Error> {
    let vagrant_path = &dir!(RESEARCH_WORKSPACE_PATH, VAGRANT_SUBDIRECTORY);

    let res = shell.run(cmd!("vagrant halt").cwd(vagrant_path));

    if res.is_err() {
        // Try again
        shell.run(cmd!("vagrant halt").cwd(vagrant_path))?;
    }

    Ok(())
}

/// Start the VM with the given amount of memory and core. If `fast` is `true`, TSC offsetting
/// is disabled during the VM boot (and re-enabled afterwards), which is much faster.
///
/// After starting the VM, we attempt to disable soft lockup detectors in the guest because they
/// can produce timing anomalies.
pub fn start_vagrant<A: std::net::ToSocketAddrs + std::fmt::Display>(
    shell: &SshShell,
    hostname: A,
    memgb: usize,
    cores: usize,
    fast: bool,
    skip_halt: bool,
    lapic_adjust: bool,
) -> Result<SshShell, failure::Error> {
    shell.run(cmd!("sudo systemctl stop firewalld"))?;
    shell.run(cmd!("sudo systemctl stop nfs-idmap.service"))?;
    shell.run(cmd!("sudo systemctl start nfs-idmap.service"))?;
    shell.run(cmd!("sudo service libvirtd restart"))?;

    // Disable KSM because it creates a lot of overhead when the host is oversubscribed
    if crate::common::service_is_running(shell, "ksm")? {
        shell.run(cmd!("sudo systemctl disable ksm"))?;
    }
    if crate::common::service_is_running(shell, "ksmtuned")? {
        shell.run(cmd!("sudo systemctl disable ksmtuned"))?;
    }

    gen_vagrantfile(shell, memgb, cores)?;

    let vagrant_path = &dir!(RESEARCH_WORKSPACE_PATH, VAGRANT_SUBDIRECTORY);

    // Make sure to turn off skip_halt, which breaks multi-core boot.
    shell.run(cmd!("echo 0 | sudo tee /proc/zerosim_skip_halt"))?;

    // Set LAPIC adjust if needed
    shell.run(cmd!(
        "echo {} | sudo tee /proc/zerosim_lapic_adjust",
        if lapic_adjust { 1 } else { 0 }
    ))?;

    // Disable TSC offsetting if `fast` is true.
    set_tsc_offsetting(shell, !fast)?;

    vagrant_halt(&shell)?;

    // We want to pin the vCPUs as soon as possible because otherwise, they tend to switch
    // around a lot, causing a lot of printk overhead.
    let pin = {
        let mut pin = HashMap::new();
        for c in 0..cores {
            pin.insert(c, c);
        }
        pin
    };
    virsh_vcpupin(shell, &pin)?;

    shell.run(cmd!("vagrant up").no_pty().cwd(vagrant_path))?;

    shell.run(cmd!("sudo lsof -i -P -n | grep LISTEN").use_bash())?;
    let vshell = connect_to_vagrant_as_root(hostname)?;

    turn_off_watchdogs(&vshell)?;

    dump_sys_info(&vshell)?;

    // Don't let the OOM killer kill ssh
    vshell.run(cmd!(
        r"pgrep -f /usr/sbin/sshd | while read PID; do \
            echo -1000 | sudo tee /proc/$PID/oom_score_adj;
        done"
    ))?;

    // Enable TSC offsetting (regardless of whether it was already off).
    set_tsc_offsetting(shell, true)?;

    // Can turn skip_halt back on now.
    if skip_halt {
        shell.run(cmd!("echo 1 | sudo tee /proc/zerosim_skip_halt"))?;
    }

    Ok(vshell)
}

/// Turn on or off 0sim TSC offsetting. Turning it off makes things run much faster, but gives up
/// accuracy. If you are doing some sort of setup routine, it is worth it to turn off.
pub fn set_tsc_offsetting(shell: &SshShell, enabled: bool) -> Result<(), failure::Error> {
    shell.run(
        cmd!(
            "echo {} | sudo tee /sys/module/kvm_intel/parameters/enable_tsc_offsetting",
            if enabled { 1 } else { 0 }
        )
        .use_bash(),
    )?;
    Ok(())
}

/// Turn off soft lockup and NMI watchdogs if possible in the shell.
pub fn turn_off_watchdogs(shell: &SshShell) -> Result<(), failure::Error> {
    shell.run(cmd!(
        "echo 0 | sudo tee /proc/sys/kernel/hung_task_timeout_secs"
    ))?;
    shell.run(cmd!("echo 0 | sudo tee /proc/sys/kernel/watchdog").allow_error())?;
    Ok(())
}

pub fn turn_off_swapdevs(shell: &SshShell) -> Result<(), failure::Error> {
    let devs = spurs_util::get_mounted_devs(shell, /* dry_run */ false)?;

    // Turn off all swap devs
    for (dev, mount) in devs {
        if mount == "[SWAP]" {
            shell.run(cmd!("sudo swapoff /dev/{}", dev))?;
        }
    }

    Ok(())
}

/// Returns a list of swap devices, with SSDs listed first.
pub fn list_swapdevs(shell: &SshShell) -> Result<Vec<String>, failure::Error> {
    let mut swapdevs = vec![];

    // Find out what swap devs are there
    let devs = spurs_util::get_unpartitioned_devs(shell, /* dry_run */ false)?;

    // Get the size of each one
    let sizes = spurs_util::get_dev_sizes(
        shell,
        devs.iter().map(String::as_str).collect(),
        /* dry_run */ false,
    )?;

    // Turn on the SSDs as swap devs
    for (dev, size) in devs.iter().zip(sizes.iter()) {
        if size == "447.1G" {
            swapdevs.push(dev.clone());
        }
    }

    // Turn on the HDDs as swap devs
    for (dev, size) in devs.iter().zip(sizes.iter()) {
        if ["1.1T", "1.8T", "2.7T", "3.7T", "931.5G"]
            .iter()
            .any(|s| s == size)
        {
            swapdevs.push(dev.clone());
        }
    }

    Ok(swapdevs)
}

/// Create and mount a thinly-partitioned swap device using device mapper. Device mapper
/// requires two devices: a metadata volume and a data volume. We use a file mounted as a
/// loopback device for the metadata volume and another arbitrary device as the data volume.
///
/// The metadata volume only needs to be a few megabytes large (e.g. 1GB would be overkill).
/// The data volume should be as large and fast as needed.
///
/// This is idempotent.
fn create_and_turn_on_thin_swap_inner(
    shell: &SshShell,
    meta_file: &str,
    data_dev: &str,
    new: bool,
) -> Result<(), failure::Error> {
    // Check if thin device is already created.
    let already = shell
        .run(cmd!("sudo dmsetup ls"))?
        .stdout
        .contains("mythin");

    if !already {
        // create loopback
        shell.run(cmd!("sudo losetup -f {}", meta_file))?;

        // find out which loopback device was created
        let out = shell.run(cmd!("sudo losetup -j {}", meta_file))?.stdout;
        let loopback = out.trim().split(':').next().expect("expected device name");

        // find out the size of the mapper_device
        let out = shell
            .run(cmd!("lsblk -o SIZE -b {} | sed '2q;d'", data_dev).use_bash())?
            .stdout;
        let mapper_device_size = out.trim().parse::<u64>().unwrap() >> 9; // 512B sectors

        // create a thin pool
        // - 0 is the start sector
        // - `mapper_device_size` is the end sector of the pool. This should be the size of the data device.
        // - `loopback` is the metadata device
        // - `mapper_device` is the data device
        // - 256000 = 128MB is the block size
        // - 0 indicates no dm event on low-watermark
        shell.run(cmd!(
            "sudo dmsetup create mypool --table \
             '0 {} thin-pool {} {} 256000 0'",
            mapper_device_size,
            loopback,
            data_dev,
        ))?;

        if new {
            // create a thin volume
            // - /dev/mapper/mypool is the name of the pool device above
            // - 0 is the sector number on the pool
            // - create_thin indicates the pool should create a new thin volume
            // - 0 is a unique 24-bit volume id
            shell.run(cmd!(
                "sudo dmsetup message /dev/mapper/mypool 0 'create_thin 0'"
            ))?;
        }

        // init the volume
        // - 0 is the start sector
        // - 21474836480 = 10TB is the end sector
        // - thin is the device type
        // - /dev/mapper/mypool is the pool to use
        // - 0 is the volume id from above
        shell.run(cmd!(
            "sudo dmsetup create mythin --table '0 21474836480 thin /dev/mapper/mypool 0'"
        ))?;

        shell.run(cmd!("sudo mkswap /dev/mapper/mythin"))?;
    }

    shell.run(cmd!("sudo swapon -d /dev/mapper/mythin"))?;

    Ok(())
}

/// Create and mount a thinly-partitioned swap device using device mapper. Device mapper
/// requires two devices: a metadata volume and a data volume. We use a file mounted as a
/// loopback device for the metadata volume and another arbitrary device as the data volume.
///
/// The metadata volume only needs to be a few megabytes large (e.g. 1GB would be overkill).
/// The data volume should be as large and fast as needed.
pub fn turn_on_thin_swap(
    shell: &SshShell,
    meta_file: &str,
    data_dev: &str,
) -> Result<(), failure::Error> {
    create_and_turn_on_thin_swap_inner(shell, meta_file, data_dev, false)
}

/// Create a new thinly-partitioned swap device using device mapper. Device mapper
/// requires two devices: a metadata volume and a data volume. We use a file mounted as a
/// loopback device for the metadata volume and another arbitrary device as the data volume.
///
/// The metadata volume only needs to be a few megabytes large (e.g. 1GB would be overkill).
/// The data volume should be as large and fast as needed.
pub fn create_thin_swap(
    shell: &SshShell,
    meta_file: &str,
    data_dev: &str,
) -> Result<(), failure::Error> {
    create_and_turn_on_thin_swap_inner(shell, meta_file, data_dev, true)
}

/// Turn on swap devices. This function will respect any `swap-devices` setting in
/// `research-settings.json`. If there are no such settings, then all unpartitioned, unmounted
/// swap devices of the right size are used (according to `list_swapdevs`).
pub fn turn_on_swapdevs(shell: &SshShell) -> Result<(), failure::Error> {
    // Find out what swap devs are there
    let settings = crate::common::get_remote_research_settings(shell)?;

    if let (Some(dm_meta), Some(dm_data)) = (
        crate::common::get_remote_research_setting(&settings, "dm-meta")?,
        crate::common::get_remote_research_setting(&settings, "dm-data")?,
    ) {
        // If a thinly-provisioned swap space is setup, load and mount it.
        return turn_on_thin_swap(shell, dm_meta, dm_data);
    }

    let devs = if let Some(devs) =
        crate::common::get_remote_research_setting(&settings, "swap-devices")?
    {
        devs
    } else {
        list_swapdevs(shell)?
    };

    // Turn on swap devs
    for dev in &devs {
        shell.run(cmd!("sudo swapon -d /dev/{}", dev))?;
    }

    shell.run(cmd!("lsblk"))?;

    Ok(())
}

/// Turn on swap devices and SSDSWAP. This function will respect any `swap-devices` setting in
/// `research-settings.json`. If there are no such settings, then all unpartitioned, unmounted
/// swap devices of the right size are used (according to `list_swapdevs`).
pub fn turn_on_ssdswap(shell: &SshShell) -> Result<(), failure::Error> {
    // Find out what swap devs are there
    let settings = crate::common::get_remote_research_settings(shell)?;
    let devs = if let Some(dm_data) =
        crate::common::get_remote_research_setting::<String>(&settings, "dm-data")?
    {
        // If the swap device in use is a thin swap
        vec![
            dm_data.replace("/dev/", ""),
            "mapper/mythin".into(),
            "mapper/mypool".into(),
        ]
    } else if let Some(devs) =
        crate::common::get_remote_research_setting(&settings, "swap-devices")?
    {
        devs
    } else {
        list_swapdevs(shell)?
    };

    // Use SSDSWAP
    for dev in &devs {
        shell.run(
            cmd!(
                "echo /dev/{} | sudo tee /sys/module/ssdswap/parameters/device",
                dev
            )
            .use_bash(),
        )?;
    }

    // Remount all swap devs
    turn_off_swapdevs(shell)?;
    turn_on_swapdevs(shell)?;

    shell.run(cmd!("lsblk -o NAME,ROTA"))?;

    Ok(())
}

/// Get the VM domain name from `virsh` for the first running VM if there is a VM running or
/// the first stopped VM if no VM is running. The `bool` returned indicates whether the VM is
/// running or not (`true` is running).
pub fn virsh_domain_name(shell: &SshShell) -> Result<(String, bool), failure::Error> {
    let running: String = shell
        .run(cmd!(
            "sudo virsh list | tail -n 2 | head -n1 | awk '{{print $2}}'"
        ))?
        .stdout
        .trim()
        .into();

    if running.is_empty() {
        Ok((
            shell
                .run(cmd!(
                    "sudo virsh list --all | tail -n 2 | head -n1 | awk '{{print $2}}'"
                ))?
                .stdout
                .trim()
                .into(),
            false,
        ))
    } else {
        Ok((running, true))
    }
}

/// For `(v, p)` in `mapping`, pin vcpu `v` to host cpu `p`. `running` indicates whether the VM
/// is running or not.
pub fn virsh_vcpupin(
    shell: &SshShell,
    mapping: &HashMap<usize, usize>,
) -> Result<(), failure::Error> {
    let (domain, running) = virsh_domain_name(shell)?;

    // We may have just changed the number of vcpus in the vagrant config, so we need to make
    // sure that libvirt is up to date.
    with_shell! { shell =>
        cmd!(
            "sudo virsh setvcpus {} {} --maximum --config",
            domain,
            mapping.len(),
        ),
        cmd!(
            "sudo virsh setvcpus {} {} --config",
            domain,
            mapping.len(),
        ),
    }

    shell.run(cmd!("sudo virsh vcpuinfo {}", domain))?;

    for (v, p) in mapping {
        shell.run(cmd!(
            "sudo virsh vcpupin {} {} {} {}",
            domain,
            if running { "" } else { "--config" },
            v,
            p
        ))?;
    }

    shell.run(cmd!("sudo virsh vcpupin {}", domain))?;

    Ok(())
}

/// Generate a Vagrantfile for a VM with the given amount of memory and number of cores. A
/// Vagrantfile should already exist containing the correct domain name.
pub fn gen_vagrantfile(shell: &SshShell, memgb: usize, cores: usize) -> Result<(), failure::Error> {
    let vagrant_path = &format!("{}/{}", RESEARCH_WORKSPACE_PATH, VAGRANT_SUBDIRECTORY);

    // Keep the same VM domain name though...
    let current_name =
        shell.run(cmd!("grep -oE ':test_vm[0-9a-zA-Z_]+' Vagrantfile").cwd(vagrant_path))?;
    let current_name = current_name.stdout.trim();

    with_shell! { shell in vagrant_path =>
        cmd!("cp Vagrantfile.bk Vagrantfile"),
        cmd!("sed -i 's/:test_vm/{}/' Vagrantfile", current_name),
        cmd!("sed -i 's/memory = 1023/memory = {}/' Vagrantfile", memgb),
        cmd!("sed -i 's/cpus = 1/cpus = {}/' Vagrantfile", cores),
    }

    let user_home = crate::common::get_user_home_dir(shell)?;
    let vagrant_full_path = &format!("{}/{}", user_home, vagrant_path).replace("/", r#"\/"#);
    let vm_shared_full_path = &format!(
        "{}/{}",
        user_home,
        crate::common::setup00000::HOSTNAME_SHARED_DIR
    )
    .replace("/", r#"\/"#);
    let research_workspace_full_path =
        &format!("{}/{}", user_home, RESEARCH_WORKSPACE_PATH).replace("/", r#"\/"#);

    with_shell! { shell in vagrant_path =>
        cmd!(
            r#"sed -i 's/vagrant_dir = ''/vagrant_dir = "{}"/' Vagrantfile"#,
            vagrant_full_path
        ),
        cmd!(
            r#"sed -i 's/vm_shared_dir = ''/vm_shared_dir = "{}"/' Vagrantfile"#,
            vm_shared_full_path
        ),
        cmd!(
            r#"sed -i 's/zerosim_workspace_dir = ''/zerosim_workspace_dir = "{}"/' Vagrantfile"#,
            research_workspace_full_path
        ),
    }

    // Choose the interface that actually gives network access. We do this by looking for the
    // interface that gives a route 1.1.1.1 (Cloudflare DNS).
    let iface = shell.run(
        cmd!(
            r#"/usr/sbin/ip route get 1.1.1.1 |\
                         grep -oE 'dev [a-z0-9]+ ' |\
                         awk '{{print $2}}'"#
        )
        .use_bash(),
    )?;
    let iface = iface.stdout.trim();

    shell.run(
        cmd!(
            r#"sed -i 's/iface = "eno1"/iface = "{}"/' Vagrantfile"#,
            iface
        )
        .cwd(vagrant_path),
    )?;
    Ok(())
}

/// Set a command line argument for the kernel. If the argument is already their, it will be
/// replaced with the new value. Otherwise, it will be appended to the list of arguments.
///
/// Requires `sudo` (obviously).
///
/// It is advised that the caller manually shutdown the guest via `sudo poweorff` to avoid
/// corruption of the guest image.
pub fn set_kernel_boot_param(
    shell: &SshShell,
    param: &str,
    value: Option<&str>,
) -> Result<(), failure::Error> {
    let current_cmd_line = shell
        .run(
            cmd!(r#"cat /etc/default/grub | grep -oP 'GRUB_CMDLINE_LINUX="\K.+(?=")'"#).use_bash(),
        )?
        .stdout;
    let current_cmd_line = current_cmd_line
        .trim()
        .replace("/", r"\/")
        .replace(r"\", r"\\");

    // Remove parameters from existing command line
    let stripped_cmd_line = current_cmd_line
        .split_whitespace()
        .filter(|p| !p.starts_with(param))
        .collect::<Vec<_>>()
        .join(" ");

    // Add the new params.
    shell.run(cmd!(
        "sudo sed -i 's/{}/{} {}/' /etc/default/grub",
        current_cmd_line,
        stripped_cmd_line,
        if let Some(value) = value {
            format!("{}={}", param, value)
        } else {
            param.into()
        }
    ))?;

    // Rebuild grub conf
    shell.run(cmd!("sudo grub2-mkconfig -o /boot/grub2/grub.cfg"))?;

    // Sync to help avoid corruption
    shell.run(cmd!("sync"))?;

    Ok(())
}

/// Gathers some common stats for any 0sim simulation. This is intended to be called after the
/// simulation.
///
/// `sim_file` should be just the file name, not the directory path. This function will cause the
/// output to be in the standard locations.
///
/// Requires `sudo`.
pub fn gen_standard_sim_output(
    sim_file: &str,
    ushell: &SshShell,
    vshell: &SshShell,
) -> Result<(), failure::Error> {
    // Get paths for the guest and host.
    let host_sim_file = dir!(setup00000::HOSTNAME_SHARED_RESULTS_DIR, sim_file);
    let guest_sim_file = dir!(setup00000::VAGRANT_RESULTS_DIR, sim_file);

    // We first gather a bunch of stats. Then, we generate a report into the given file.

    // Host config
    ushell.run(cmd!("echo -e 'Host Config\n=====' > {}", host_sim_file))?;
    ushell.run(cmd!("cat /proc/cpuinfo >> {}", host_sim_file))?;
    ushell.run(cmd!("lsblk >> {}", host_sim_file))?;

    // Memory usage, compressibility
    ushell.run(cmd!(
        "echo -e '\nSimulation Stats (Host)\n=====' >> {}",
        host_sim_file
    ))?;
    ushell.run(cmd!("cat /proc/meminfo >> {}", host_sim_file))?;
    ushell.run(cmd!(
        "sudo bash -c 'tail /sys/kernel/debug/zswap/*' >> {}",
        host_sim_file
    ))?;
    ushell.run(cmd!(
        "(tail /proc/zerosim_guest_offset; echo) >> {}",
        host_sim_file
    ))?;

    ushell.run(cmd!("sync"))?;
    vshell.run(cmd!(
        "echo -e '\nSimulation Stats (Guest)\n=====' >> {}",
        guest_sim_file
    ))?;
    vshell.run(cmd!("cat /proc/meminfo >> {}", guest_sim_file))?;

    vshell.run(cmd!("sync"))?;
    ushell.run(cmd!("sync"))?;

    ushell.run(cmd!("echo -e '\ndmesg (Host)\n=====' >> {}", host_sim_file))?;
    ushell.run(cmd!("dmesg >> {}", host_sim_file))?;

    vshell.run(cmd!("sync"))?;
    ushell.run(cmd!("sync"))?;

    vshell.run(cmd!(
        "echo -e '\ndmesg (Guest)\n=====' >> {}",
        guest_sim_file
    ))?;
    vshell.run(cmd!("dmesg >> {}", guest_sim_file))?;

    vshell.run(cmd!("sync"))?;
    ushell.run(cmd!("sync"))?;

    Ok(())
}
