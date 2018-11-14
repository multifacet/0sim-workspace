//! Setup the given cloudlab node for vagrant via libvirt and install a custom kernel from source.
//! This does not set up the guest -- only the host. It allows formatting and setting up a device
//! as the home directory of the given user. It also allows choosing the git branch to compile the
//! kernel from.

use std::process::Command;

use spurs::{cmd, ssh::SshShell};

const VAGRANT_RPM_URL: &str =
    "https://releases.hashicorp.com/vagrant/2.1.5/vagrant_2.1.5_x86_64.rpm";

const DEFAULT_GIT_BRANCH: &str = "markm_ztier";

pub fn run<A: std::net::ToSocketAddrs + std::fmt::Display>(
    dry_run: bool,
    cloudlab: A,
    username: &str,
    device: Option<&str>,
    git_branch: Option<&str>,
) -> Result<(), failure::Error> {
    // Connect to the remote
    let mut ushell = SshShell::with_default_key(username, &cloudlab)?;
    if dry_run {
        ushell.toggle_dry_run();
    }

    // Install a bunch of stuff
    ushell.run(spurs::centos::yum_install(&[
        "libvirt",
        "libvirt-devel",
        "qemu-kvm",
        "virt-manager",
        "pciutils-devel",
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
    let mut ushell = SshShell::with_default_key(username, &cloudlab)?;
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
            &format!("/users/{}", username),
            username,
        )?;
    }

    // Setup all other devices as swap devices
    let unpartitioned = spurs::util::get_unpartitioned_devs(&ushell, dry_run)?;
    for dev in unpartitioned.iter() {
        ushell.run(cmd!("sudo mkswap /dev/{}", dev))?;
    }

    // clone linux-dev
    ushell.run(cmd!("mkdir -p linux-dev"))?;
    ushell.run(cmd!("git init").cwd(&format!("/users/{}/linux-dev", username)))?;
    ushell.run(
        cmd!("git checkout -b side")
            .cwd(&format!("/users/{}/linux-dev", username))
            .allow_error(), // if already exists
    )?;

    let git_branch = if let Some(git_branch) = git_branch {
        git_branch
    } else {
        DEFAULT_GIT_BRANCH
    };

    if !dry_run {
        let _ = Command::new("git")
            .args(&["checkout", git_branch])
            .current_dir("/u/m/a/markm/private/large_mem/software/linux-dev")
            .status()?;

        let _ = Command::new("git")
            .args(&[
                "push",
                "-u",
                &format!("ssh://{}/users/{}/linux-dev", cloudlab, username),
                git_branch,
            ])
            .current_dir("/u/m/a/markm/private/large_mem/software/linux-dev")
            .status()?;
    }
    ushell
        .run(cmd!("git checkout {}", git_branch).cwd(&format!("/users/{}/linux-dev", username)))?;

    // compile linux-dev
    ushell.run(cmd!("mkdir -p /users/{}/linux-dev/kbuild", username))?;
    ushell.run(
        cmd!("make O=/users/{}/linux-dev/kbuild defconfig", username)
            .cwd(&format!("/users/{}/linux-dev", username)),
    )?;
    let config = ushell
        .run(cmd!("ls -1 /boot/config-* | head -n1").use_bash())?
        .stdout;
    let config = config.trim();
    ushell.run(cmd!(
        "cp {} /users/{}/linux-dev/kbuild/.config",
        config,
        username
    ))?;
    ushell.run(
        cmd!("yes '' | make oldconfig")
            .use_bash()
            .cwd(&format!("/users/{}/linux-dev/kbuild", username)),
    )?;

    // make sure some configurations are set/not set
    const CONFIG_SET: &[&str] = &[
        "CONFIG_ZSWAP",
        "CONFIG_ZPOOL",
        "CONFIG_ZBUD",
        "CONFIG_ZTIER",
        "CONFIG_SBALLOC",
        "CONFIG_ZSMALLOC",
    ];
    for opt in CONFIG_SET.iter() {
        ushell.run(cmd!(
            "sed -i 's/# {} is not set/{}=y/' /users/{}/linux-dev/kbuild/.config",
            opt,
            opt,
            username
        ))?;
    }

    let nprocess = ushell.run(cmd!("getconf _NPROCESSORS_ONLN"))?.stdout;
    let nprocess = nprocess.trim();
    ushell.run(
        cmd!("make -j {} binrpm-pkg LOCALVERSION=-ztier", nprocess)
            .cwd(&format!("/users/{}/linux-dev/kbuild", username))
            .allow_error(),
    )?;
    ushell.run(
        cmd!("make -j {} binrpm-pkg LOCALVERSION=-ztier", nprocess)
            .cwd(&format!("/users/{}/linux-dev/kbuild", username)),
    )?;

    // install linux-dev
    ushell.run(
        cmd!("sudo yum -y install `ls -t1 | head -n2 | sort`")
            .use_bash()
            .cwd(&format!("/users/{}/rpmbuild/RPMS/x86_64/", username)),
    )?;

    // update grub to choose this entry (new kernel) by default
    ushell.run(cmd!("sudo grub2-set-default 0"))?;

    // change image location
    ushell.run(cmd!("mkdir -p vm_shared/results/"))?;
    ushell.run(cmd!("mkdir -p images"))?;
    ushell.run(cmd!(
        "sudo ln -sf /var/lib/libvirt/images/ /users/{}/images",
        username
    ))?;

    spurs::util::reboot(&mut ushell, dry_run)?;

    // Add ssh key to VM
    super::exp00000::gen_vagrantfile_gb(&ushell, 20)?;
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

    // Start vagrant
    let vshell = super::exp00000::start_vagrant(&ushell, &cloudlab, 20)?;

    // Install stuff on the VM
    vshell.run(spurs::centos::yum_install(&[
        "vim",
        "git",
        "memcached",
        "gcc",
    ]))?;

    vshell.run(cmd!("curl https://sh.rustup.rs -sSf | sh -s -- --default-toolchain nightly --no-modify-path -y").use_bash().no_pty())?;

    // Install benchmarks
    vshell.run(cmd!("mkdir -p paperexp").cwd("/home/vagrant"))?;
    vshell.run(cmd!("git init").cwd("/home/vagrant/paperexp"))?;
    vshell.run(
        cmd!("git checkout -b side")
            .cwd("/home/vagrant/paperexp")
            .allow_error(), // if already exists
    )?;

    if !dry_run {
        let (host, _) = spurs::util::get_host_ip(cloudlab);

        let _ = Command::new("git")
            .args(&["checkout", "master"])
            .current_dir("/u/m/a/markm/private/large_mem/tools/paperexp/")
            .status()?;

        let _ = Command::new("git")
            .args(&[
                "push",
                "-u",
                &format!(
                    "ssh://root@{}:{}/home/vagrant/paperexp",
                    host,
                    super::exp00000::VAGRANT_PORT
                ),
                "master",
            ])
            .current_dir("/u/m/a/markm/private/large_mem/tools/paperexp/")
            .status()?;
    }
    vshell.run(cmd!("git checkout master").cwd("/home/vagrant/paperexp"))?;

    vshell.run(cmd!("/root/.cargo/bin/cargo build --release").cwd("/home/vagrant/paperexp"))?;

    Ok(())
}
