//! A library of routines commonly used in experiments.

#[derive(Copy, Clone, Debug)]
pub struct Username<'u>(pub &'u str);

impl Username<'_> {
    pub fn as_str(&self) -> &str {
        self.0
    }
}

pub struct Login<'u, A: std::net::ToSocketAddrs + std::fmt::Display> {
    pub host: A,
    pub username: Username<'u>,
}

pub mod setup00000 {
    use std::process::Command;

    use spurs::{cmd, ssh::SshShell};

    pub use super::{Login, Username};

    /// Build a Linux kernel RPM on the remote host using the given kernel branch and kernel build
    /// config options.
    ///
    /// `config_options` is a list of config option names that should be set to `y` before
    /// building. It is the caller's responsibility to make sure that all dependencies are on too.
    ///
    /// `kernel_local_version` is the kernel `LOCALVERSION` string to pass to `make` for the RPM.
    pub fn build_kernel_rpm<A: std::net::ToSocketAddrs + std::fmt::Display>(
        dry_run: bool,
        ushell: &SshShell,
        login: &Login<A>,
        git_branch: &str,
        config_options: &[&str],
        kernel_local_version: &str,
    ) -> Result<(), failure::Error> {
        ushell.run(cmd!("mkdir -p linux-dev"))?;
        ushell
            .run(cmd!("git init").cwd(&format!("/users/{}/linux-dev", login.username.as_str())))?;
        ushell.run(
            cmd!("git checkout -b side")
                .cwd(&format!("/users/{}/linux-dev", login.username.as_str()))
                .allow_error(), // if already exists
        )?;

        if !dry_run {
            let _ = Command::new("git")
                .args(&["checkout", git_branch])
                .current_dir("/u/m/a/markm/private/large_mem/software/linux-dev")
                .status()?;

            let _ = Command::new("git")
                .args(&[
                    "push",
                    "-u",
                    &format!(
                        "ssh://{}/users/{}/linux-dev",
                        login.host,
                        login.username.as_str()
                    ),
                    git_branch,
                ])
                .current_dir("/u/m/a/markm/private/large_mem/software/linux-dev")
                .status()?;
            let _ = Command::new("git")
                .args(&["checkout", "side"])
                .current_dir("/u/m/a/markm/private/large_mem/software/linux-dev")
                .status()?;
        }
        ushell.run(
            cmd!("git checkout {}", git_branch)
                .cwd(&format!("/users/{}/linux-dev", login.username.as_str())),
        )?;

        // compile linux-dev
        ushell.run(cmd!(
            "mkdir -p /users/{}/linux-dev/kbuild",
            login.username.as_str()
        ))?;

        // save old config if there is one
        ushell.run(
            cmd!("cp .config config.bak",)
                .cwd(&format!(
                    "/users/{}/linux-dev/kbuild",
                    login.username.as_str()
                ))
                .allow_error(),
        )?;

        ushell.run(
            cmd!(
                "make O=/users/{}/linux-dev/kbuild defconfig",
                login.username.as_str()
            )
            .cwd(&format!("/users/{}/linux-dev", login.username.as_str())),
        )?;
        let config = ushell
            .run(cmd!("ls -1 /boot/config-* | head -n1").use_bash())?
            .stdout;
        let config = config.trim();
        ushell.run(cmd!(
            "cp {} /users/{}/linux-dev/kbuild/.config",
            config,
            login.username.as_str(),
        ))?;
        ushell.run(cmd!("yes '' | make oldconfig").use_bash().cwd(&format!(
            "/users/{}/linux-dev/kbuild",
            login.username.as_str()
        )))?;

        // make sure some configurations are set
        for opt in config_options.iter() {
            ushell.run(cmd!(
                "sed -i 's/# {} is not set/{}=y/' /users/{}/linux-dev/kbuild/.config",
                opt,
                opt,
                login.username.as_str()
            ))?;
        }

        let nprocess = ushell.run(cmd!("getconf _NPROCESSORS_ONLN"))?.stdout;
        let nprocess = nprocess.trim();
        ushell.run(
            cmd!(
                "make -j {} binrpm-pkg LOCALVERSION=-{}",
                nprocess,
                kernel_local_version
            )
            .cwd(&format!(
                "/users/{}/linux-dev/kbuild",
                login.username.as_str()
            ))
            .allow_error(),
        )?;
        ushell.run(
            cmd!(
                "make -j {} binrpm-pkg LOCALVERSION=-{}",
                nprocess,
                kernel_local_version
            )
            .cwd(&format!(
                "/users/{}/linux-dev/kbuild",
                login.username.as_str()
            )),
        )?;

        Ok(())
    }
}

pub mod exp00000 {
    use std::collections::HashMap;

    use spurs::{cmd, ssh::SshShell};

    pub use super::{Login, Username};

    /// The port that vagrant VMs forward from.
    pub const VAGRANT_PORT: u16 = 5555;

    /// The default amount of memory of the VM.
    pub const VAGRANT_MEM: usize = 1024;

    /// The default number of cores of the VM.
    pub const VAGRANT_CORES: usize = 1;

    pub fn run_setup_only<A: std::net::ToSocketAddrs + std::fmt::Display>(
        dry_run: bool,
        login: &Login<A>,
        vm_size: Option<usize>,
        cores: Option<usize>,
    ) -> Result<(), failure::Error> {
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
        let _ = connect_and_setup_host_and_vagrant(dry_run, &login, vm_size, cores)?;

        Ok(())
    }

    /// Reboot the machine and do nothing else. Useful for getting the machine into a clean state.
    pub fn initial_reboot<A: std::net::ToSocketAddrs + std::fmt::Display>(
        dry_run: bool,
        login: &Login<A>,
    ) -> Result<(), failure::Error> {
        // Connect to the remote
        let mut ushell = SshShell::with_default_key(login.username.as_str(), &login.host)?;
        if dry_run {
            ushell.toggle_dry_run();
        }

        // Reboot the remote to make sure we have a clean slate
        spurs::util::reboot(&mut ushell, dry_run)?;

        Ok(())
    }

    /// Connects to the host and to vagrant. Returns shells for both.
    pub fn connect_and_setup_host_and_vagrant<A: std::net::ToSocketAddrs + std::fmt::Display>(
        dry_run: bool,
        login: &Login<A>,
        vm_size: usize,
        cores: usize,
    ) -> Result<(SshShell, SshShell), failure::Error> {
        let ushell = connect_and_setup_host_only(dry_run, &login)?;
        let vshell = start_vagrant(&ushell, &login.host, vm_size, cores)?;

        Ok((ushell, vshell))
    }

    /// Connects to the host, waiting for it to come up if necessary. Turn on only the swap devices we
    /// want. Set the scaling governor. Returns the shell to the host.
    pub fn connect_and_setup_host_only<A: std::net::ToSocketAddrs + std::fmt::Display>(
        dry_run: bool,
        login: &Login<A>,
    ) -> Result<SshShell, failure::Error> {
        // Keep trying to connect until we succeed
        let ushell = {
            let mut shell;
            loop {
                shell = match SshShell::with_default_key(login.username.as_str(), &login.host) {
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

        ushell.run(cmd!("sudo /users/markm/linux-dev/tools/power/cpupower/cpupower frequency-set -g performance").dry_run(dry_run))?;

        ushell.run(cmd!("echo 4 | sudo tee /proc/sys/kernel/printk").use_bash())?;

        Ok(ushell)
    }

    /// Turn on Zswap with some default parameters.
    pub fn turn_on_zswap(shell: &mut SshShell, dry_run: bool) -> Result<(), failure::Error> {
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
        cores: usize,
    ) -> Result<SshShell, failure::Error> {
        shell.run(cmd!("sudo systemctl stop firewalld"))?;
        shell.run(cmd!("sudo systemctl stop nfs-idmap.service"))?;
        shell.run(cmd!("sudo systemctl start nfs-idmap.service"))?;

        gen_vagrantfile(shell, memgb, cores)?;

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
            for c in 0..cores {
                pin.insert(c, c);
            }
            pin
        };
        virsh_vcpupin(shell, &pin)?;

        Ok(vshell)
    }

    pub fn turn_off_swapdevs(shell: &SshShell, dry_run: bool) -> Result<(), failure::Error> {
        let devs = spurs::util::get_mounted_devs(shell, dry_run)?;

        // Turn off all swap devs
        for (dev, mount) in devs {
            if mount == "[SWAP]" {
                shell.run(cmd!("sudo swapoff /dev/{}", dev))?;
            }
        }

        Ok(())
    }

    pub fn turn_on_swapdevs(shell: &SshShell, dry_run: bool) -> Result<(), failure::Error> {
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
    pub fn virsh_vcpupin(
        shell: &SshShell,
        mapping: &HashMap<usize, usize>,
    ) -> Result<(), failure::Error> {
        shell.run(cmd!("sudo virsh vcpuinfo markm_vagrant_test_vm"))?;

        for (v, p) in mapping {
            shell.run(cmd!("sudo virsh vcpupin markm_vagrant_test_vm {} {}", v, p))?;
        }

        shell.run(cmd!("sudo virsh vcpuinfo markm_vagrant_test_vm"))?;

        Ok(())
    }

    /// Generate a Vagrantfile for a VM with the given amount of memory and number of cores.
    pub fn gen_vagrantfile(
        shell: &SshShell,
        memgb: usize,
        cores: usize,
    ) -> Result<(), failure::Error> {
        shell.run(
            cmd!("cp Vagrantfile.bk Vagrantfile").cwd("/proj/superpages-PG0/markm_vagrant/"),
        )?;
        shell.run(
            cmd!("sed -i 's/memory = 1023/memory = {}/' Vagrantfile", memgb)
                .cwd("/proj/superpages-PG0/markm_vagrant/"),
        )?;
        shell.run(
            cmd!("sed -i 's/cpus = 1/cpus = {}/' Vagrantfile", cores)
                .cwd("/proj/superpages-PG0/markm_vagrant/"),
        )?;
        Ok(())
    }

}

pub mod exp00001 {
    use std::collections::HashMap;

    use spurs::{cmd, ssh::SshShell};

    pub use super::exp00000::{connect_to_vagrant, VAGRANT_CORES, VAGRANT_PORT};
    pub use super::{Login, Username};

    /// The default amount of memory of the VM.
    pub const VAGRANT_MEM: usize = 1023;

    /// Turn on Zswap with some default parameters.
    pub fn turn_on_zswap(shell: &mut SshShell, dry_run: bool) -> Result<(), failure::Error> {
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

    /// For `(v, p)` in `mapping`, pin vcpu `v` to host cpu `p`.
    pub fn virsh_vcpupin(
        shell: &SshShell,
        mapping: &HashMap<usize, usize>,
    ) -> Result<(), failure::Error> {
        shell.run(cmd!("virsh vcpuinfo vagrant_test_vm"))?;

        for (v, p) in mapping {
            shell.run(cmd!("virsh vcpupin vagrant_test_vm {} {}", v, p))?;
        }

        shell.run(cmd!("virsh vcpuinfo vagrant_test_vm"))?;

        Ok(())
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
    pub fn initial_reboot<A: std::net::ToSocketAddrs + std::fmt::Display>(
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
    pub fn connect_and_setup_host_and_vagrant<A: std::net::ToSocketAddrs + std::fmt::Display>(
        dry_run: bool,
        login: &Login<A>,
    ) -> Result<(SshShell, SshShell, SshShell), failure::Error> {
        let (ushell, rshell) = connect_and_setup_host_only(dry_run, &login)?;
        let vshell = start_vagrant(&ushell, &login.host, VAGRANT_MEM)?;

        Ok((ushell, rshell, vshell))
    }

    /// Connects to the host, waiting for it to come up if necessary. Turn on only the swap devices we
    /// want. Set the scaling governor. Returns the shell to the host.
    pub fn connect_and_setup_host_only<A: std::net::ToSocketAddrs + std::fmt::Display>(
        dry_run: bool,
        login: &Login<A>,
    ) -> Result<(SshShell, SshShell), failure::Error> {
        // Keep trying to connect until we succeed
        let rshell = {
            let mut shell;
            loop {
                shell = match SshShell::with_default_key("root", &login.host) {
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
                shell = match SshShell::with_default_key(login.username.as_str(), &login.host) {
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
            cmd!(
                "/home/markm/linux-dev/tools/power/cpupower/cpupower frequency-set -g performance"
            )
            .dry_run(dry_run),
        )?;

        rshell.run(cmd!("echo 4 > /proc/sys/kernel/printk").use_bash())?;

        Ok((ushell, rshell))
    }
}

pub mod exp00002 {
    pub use super::exp00000::{
        connect_and_setup_host_and_vagrant, connect_and_setup_host_only, connect_to_vagrant,
        gen_vagrantfile, initial_reboot, run_setup_only, start_vagrant, turn_off_swapdevs,
        turn_on_swapdevs, turn_on_zswap, virsh_vcpupin, VAGRANT_CORES, VAGRANT_MEM, VAGRANT_PORT,
    };
    pub use super::{Login, Username};
}

pub mod exp00003 {
    pub use super::exp00000::*;
}

pub mod exp00004 {
    pub use super::exp00003::*;
}
