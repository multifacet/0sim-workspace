//! Setup the given cloudlab node for vagrant via libvirt and install a custom kernel from source.
//! This does not set up the guest -- only the host. It allows formatting and setting up a device
//! as the home directory of the given user. It also allows choosing the git branch to compile the
//! kernel from.

use std::process::Command;

use spurs::{
    cmd,
    ssh::{Execute, SshShell},
};

use crate::common::Login;

const VAGRANT_RPM_URL: &str =
    "https://releases.hashicorp.com/vagrant/2.1.5/vagrant_2.1.5_x86_64.rpm";

pub fn run<A>(
    dry_run: bool,
    login: &Login<A>,
    device: Option<&str>,
    git_branch: Option<&str>,
    only_vm: bool,
) -> Result<(), failure::Error>
where
    A: std::net::ToSocketAddrs + std::fmt::Display + std::fmt::Debug,
{
    // Connect to the remote
    let mut ushell = SshShell::with_default_key(login.username.as_str(), &login.host)?;
    if dry_run {
        ushell.toggle_dry_run();
    }

    if !only_vm {
        // Rename `poweroff` so we can't accidentally use it
        ushell.run(
            cmd!(
                "type poweroff && sudo mv $(type poweroff | awk '{{print $3}}') \
                 /usr/sbin/poweroff-actually || echo already renamed"
            )
            .use_bash(),
        )?;

        // Install a bunch of stuff
        ushell.run(spurs::centos::yum_install(&[
            "libvirt",
            "libvirt-devel",
            "qemu-kvm",
            "virt-manager",
            "pciutils-devel",
            "bash-completion",
        ]))?;
        ushell.run(spurs::util::add_to_group("libvirt"))?;

        let installed = ushell
            .run(cmd!("yum list installed vagrant | grep -q vagrant"))
            .is_ok();

        if !installed {
            ushell.run(cmd!("sudo yum -y install {}", VAGRANT_RPM_URL))?;
        }

        ushell.run(cmd!("vagrant plugin install vagrant-libvirt"))?;

        // Need a new shell so that we get the new user group
        let mut ushell = SshShell::with_default_key(login.username.as_str(), &login.host)?;
        if dry_run {
            ushell.toggle_dry_run();
        }

        if let Some(device) = device {
            // Set up home device/directory
            // - format the device and create a partition
            // - mkfs on the partition
            // - copy data to new partition and mount as home dir
            ushell.run(spurs::util::write_gpt(device))?;
            ushell.run(spurs::util::create_partition(device))?;
            spurs::util::format_partition_as_ext4(
                &ushell,
                dry_run,
                &format!("{}1", device), // assume it is the first device partition
                &format!("/users/{}", login.username.as_str()),
                login.username.as_str(),
            )?;
        }

        // Setup all other devices as swap devices
        let unpartitioned = spurs::util::get_unpartitioned_devs(&ushell, dry_run)?;
        for dev in unpartitioned.iter() {
            ushell.run(cmd!("sudo mkswap /dev/{}", dev))?;
        }

        // clone linux-dev
        if let Some(git_branch) = git_branch {
            const CONFIG_SET: &[(&str, bool)] = &[
                ("CONFIG_ZSWAP", true),
                ("CONFIG_ZPOOL", true),
                ("CONFIG_ZBUD", true),
                ("CONFIG_ZTIER", true),
                ("CONFIG_SBALLOC", true),
                ("CONFIG_ZSMALLOC", true),
                ("CONFIG_PAGE_TABLE_ISOLATION", false),
                ("CONFIG_RETPOLINE", false),
            ];

            crate::common::setup00000::build_kernel_rpm(
                dry_run, &ushell, login, git_branch, CONFIG_SET, "ztier",
            )?;

            // Build cpupower
            ushell.run(
                cmd!("make")
                    .cwd("/users/markm/linux-dev/tools/power/cpupower/")
                    .dry_run(dry_run),
            )?;

            // install linux-dev
            ushell.run(
                cmd!("sudo yum -y install `ls -t1 | head -n2 | sort`")
                    .use_bash()
                    .cwd(&format!(
                        "/users/{}/rpmbuild/RPMS/x86_64/",
                        login.username.as_str()
                    )),
            )?;

            // update grub to choose this entry (new kernel) by default
            ushell.run(cmd!("sudo grub2-set-default 0"))?;
        }

        // change image location
        ushell.run(cmd!("mkdir -p vm_shared/results/"))?;
        ushell.run(cmd!("mkdir -p images"))?;
        ushell.run(cmd!(
            "sudo ln -sf /users/{}/images /var/lib/libvirt/images",
            login.username.as_str()
        ))?;

        spurs::util::reboot(&mut ushell, dry_run)?;
    }

    // Add ssh key to VM
    crate::common::exp00000::gen_vagrantfile(&ushell, 20, 1)?;
    ushell.run(cmd!("vagrant halt").cwd("/proj/superpages-PG0/markm_vagrant"))?;
    ushell.run(cmd!("vagrant up").cwd("/proj/superpages-PG0/markm_vagrant"))?;

    let key = std::fs::read_to_string("/u/m/a/markm/.ssh/id_rsa.pub")?;
    let key = key.trim();
    ushell.run(
        cmd!(
            "vagrant ssh -- 'echo {} >> /home/vagrant/.ssh/authorized_keys'",
            key
        )
        .cwd("/proj/superpages-PG0/markm_vagrant"),
    )?;
    ushell.run(
        cmd!("vagrant ssh -- sudo cp -r /home/vagrant/.ssh /root/")
            .cwd("/proj/superpages-PG0/markm_vagrant"),
    )?;

    // Old key will be cached for the VM, but it is likely to have changed
    let (host, _) = spurs::util::get_host_ip(&login.host);
    let _ = Command::new("ssh-keygen")
        .args(&[
            "-f",
            "/u/m/a/markm/.ssh/known_hosts",
            "-R",
            &format!("[{}]:{}", host, crate::common::exp00000::VAGRANT_PORT),
        ])
        .status()
        .unwrap();

    // Start vagrant
    let vshell = crate::common::exp00000::start_vagrant(&ushell, &login.host, 20, 1)?;

    // Install stuff on the VM
    vshell.run(spurs::centos::yum_install(&[
        "vim",
        "git",
        "memcached",
        "gcc",
        "libcgroup",
        "libcgroup-tools",
    ]))?;

    vshell.run(cmd!("curl https://sh.rustup.rs -sSf | sh -s -- --default-toolchain nightly --no-modify-path -y").use_bash().no_pty())?;

    // Install benchmarks
    vshell.run(
        cmd!(
            "git clone {} 0sim-experiments",
            crate::common::setup00000::ZEROSIM_EXPERIMENTS_SRC_REPO
        )
        .cwd("/home/vagrant/"),
    )?;

    vshell.run(
        cmd!("/root/.cargo/bin/cargo build --release").cwd("/home/vagrant/0sim-experiments"),
    )?;

    Ok(())
}
