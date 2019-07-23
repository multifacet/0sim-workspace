//! Setup the given test node for vagrant via libvirt and install a custom kernel from source.
//! This does not set up the guest -- only the host. It allows formatting and setting up a device
//! as the home directory of the given user. It also allows choosing the git branch to compile the
//! kernel from.

use std::process::Command;

use clap::clap_app;

use spurs::{
    cmd,
    ssh::{Execute, SshShell},
};

use crate::common::{
    exp_0sim::*,
    get_user_home_dir,
    paths::{setup00000::*, *},
    KernelBaseConfigSource, KernelConfig, KernelPkgType, KernelSrc, Login, ServiceAction, Username,
};

const VAGRANT_RPM_URL: &str =
    "https://releases.hashicorp.com/vagrant/2.1.5/vagrant_2.1.5_x86_64.rpm";

const HADOOP_TARBALL: &str =
    "http://apache.cs.utah.edu/hadoop/common/hadoop-3.1.2/hadoop-3.1.2.tar.gz";
const HADOOP_TARBALL_NAME: &str = "hadoop-3.1.2.tar.gz";
const HADOOP_HOME: &str = "hadoop-3.1.2";

const SPARK_TARBALL: &str =
    "http://apache.cs.utah.edu/spark/spark-2.4.3/spark-2.4.3-bin-hadoop2.7.tgz";
const SPARK_TARBALL_NAME: &str = "spark-2.4.3-bin-hadoop2.7.tgz";
const SPARK_HOME: &str = "spark-2.4.3-bin-hadoop2.7";

const QEMU_TARBALL: &str = "https://download.qemu.org/qemu-4.0.0.tar.xz";
const QEMU_TARBALL_NAME: &str = "qemu-4.0.0.tar.xz";

/// Location of `.ssh` directory on UW CS AFS so we can install it on experimental machines.
const SSH_LOCATION: &str = "/u/m/a/markm/.ssh";

pub fn cli_options() -> clap::App<'static, 'static> {
    clap_app! { setup00000 =>
        (about: "Sets up the given _centos_ test machine for use with vagrant. Requires `sudo`.")
        (@arg HOSTNAME: +required +takes_value
         "The domain name of the remote (e.g. c240g2-031321.wisc.cloudlab.us:22)")
        (@arg USERNAME: +required +takes_value
         "The username on the remote (e.g. markm)")
        (@arg TOKEN: +required +takes_value
         "This is the Github personal token for cloning the repo.")
        (@arg DEVICE: +takes_value -d --device
         "(Optional) the device to format and use as a home directory (e.g. -d /dev/sda)")
        (@arg MAPPER_DEVICE: +takes_value -m --mapper_device
         "(Optional) the device to use with device mapper as a thinly-provisioned swap space (e.g. -d /dev/sda)")
        (@arg GIT_BRANCH: +takes_value -g --git_branch
         "(Optional) the git branch to compile the kernel from (e.g. -g markm_ztier)")
        (@arg ONLY_VM: -v --only_vm
         "(Optional) only setup the VM")
        (@arg SWAP_DEV: -s --swap +takes_value ...
         "(Optional) specify which devices to use as swap devices. By default all \
          unpartitioned, unmounted devices are used (e.g. -s sda sdb sdc).")
        (@arg DISABLE_EPT: --disable_ept
         "(Optional) may need to disable Intel EPT on machines that don't have enough physical bits.")
        (@arg HADOOP: --hadoop
         "(Optional) set up hadoop stack on VM.")
        (@arg PROXY: -p --proxy +takes_value
         "(Optional) set up the VM to use the given proxy. Leave off the protocol (e.g. squid.cs.wisc.edu:3128)")
        (@arg DESTROY_EXISTING: --DESTROY_EXISTING
         "(Optional) destroy any existing VM before starting a new VM")
        (@arg SKIP_VM_KERNEL: --skip_vm_kernel
         "(Optional) don't build a guest kernel")
    }
}

struct SetupConfig<'a, A>
where
    A: std::net::ToSocketAddrs + std::fmt::Display + std::fmt::Debug + Clone,
{
    login: Login<'a, 'a, A>,
    device: Option<&'a str>,
    mapper_device: Option<&'a str>,
    git_branch: Option<&'a str>,
    only_vm: bool,
    token: &'a str,
    swap_devs: Vec<&'a str>,
    disable_ept: bool,
    setup_hadoop: bool,
    setup_proxy: Option<&'a str>,
    destroy_existing_vm: bool,
    skip_vm_kernel: bool,
    dry_run: bool,
}

pub fn run(dry_run: bool, sub_m: &clap::ArgMatches<'_>) -> Result<(), failure::Error> {
    let login = Login {
        username: Username(sub_m.value_of("USERNAME").unwrap()),
        hostname: sub_m.value_of("HOSTNAME").unwrap(),
        host: sub_m.value_of("HOSTNAME").unwrap(),
    };
    let device = sub_m.value_of("DEVICE");
    let mapper_device = sub_m.value_of("MAPPER_DEVICE");
    let git_branch = sub_m.value_of("GIT_BRANCH");
    let only_vm = sub_m.is_present("ONLY_VM");
    let token = sub_m.value_of("TOKEN").unwrap();
    let swap_devs = sub_m
        .values_of("SWAP_DEV")
        .map(|i| i.collect())
        .unwrap_or_else(|| vec![]);
    let disable_ept = sub_m.is_present("DISABLE_EPT");
    let setup_hadoop = sub_m.is_present("HADOOP");
    let setup_proxy = sub_m.value_of("PROXY");
    let destroy_existing_vm = sub_m.is_present("DESTROY_EXISTING");
    let skip_vm_kernel = sub_m.is_present("SKIP_VM_KERNEL");

    assert!(mapper_device.is_none() || swap_devs.is_empty());

    let mut cfg = SetupConfig {
        dry_run,
        login,
        device,
        mapper_device,
        git_branch,
        only_vm,
        token,
        swap_devs,
        disable_ept,
        setup_proxy,
        setup_hadoop,
        destroy_existing_vm,
        skip_vm_kernel,
    };

    // Connect to the remote
    let mut ushell = SshShell::with_default_key(cfg.login.username.as_str(), &cfg.login.host)?;
    ushell.set_dry_run(dry_run);

    if !cfg.only_vm {
        set_up_host(&mut ushell, &mut cfg)?;
    }

    // Create the VM and install dependencies for the benchmarks/simulator.
    let (vrshell, vushell) = init_vm(&mut ushell, &cfg)?;

    // We share the research-workspace with the VM via a vagrant shared directory (NFS) so that
    // there is only one version used across both (less versioning to track). Now, just compile the
    // benchmarks and install rust on the host.

    if !cfg.skip_vm_kernel {
        install_guest_kernel(&ushell, &vrshell, &vushell, &cfg)?;
    }

    // Install benchmarks.
    install_benchmarks(&ushell, &vrshell, &cfg)?;

    // Make sure the TSC is marked as a reliable clock source in the guest.
    set_kernel_boot_param(&vrshell, "tsc", Some("reliable"))?;

    // Need to run shutdown to make sure that the next host reboot doesn't lose guest data.
    vrshell.run(cmd!("sync"))?;
    ushell.run(cmd!("sync"))?;
    let _ = vrshell.run(cmd!("sudo poweroff")); // This will give a TCP error for obvious reasons

    Ok(())
}

fn set_up_host<A>(ushell: &mut SshShell, cfg: &mut SetupConfig<'_, A>) -> Result<(), failure::Error>
where
    A: std::net::ToSocketAddrs + std::fmt::Display + std::fmt::Debug + Clone,
{
    let user_home = &get_user_home_dir(&ushell)?;

    // Rename `poweroff` so we can't accidentally use it
    if let Ok(res) = ushell.run(
        cmd!("PATH='/usr/local/bin:/usr/bin:/usr/local/sbin:/usr/sbin':$PATH type poweroff")
            .use_bash(),
    ) {
        ushell.run(
            cmd!(
                "sudo mv $(echo '{}' | awk '{{print $3}}') /usr/sbin/poweroff-actually",
                res.stdout.trim()
            )
            .use_bash(),
        )?;
    }

    // Install a bunch of stuff
    with_shell! { ushell =>
        cmd!("sudo yum group install -y 'Development Tools'"),
        spurs_util::centos::yum_install(&[
            "bc",
            "openssl-devel",
            "libvirt",
            "libvirt-devel",
            "virt-manager",
            "pciutils-devel",
            "bash-completion",
            "elfutils-devel",
            "libunwind-devel",
            "audit-libs-devel",
            "slang-devel",
            "perl-ExtUtils-Embed",
            "binutils-devel",
            "xz-devel",
            "numactl-devel",
            "lsof",
            "java-1.8.0-openjdk",
            "centos-release-scl",
            "scl-utils",
            "maven",
            "glib2-devel",
            "libfdt-devel",
            "pixman-devel",
            "zlib-devel",
            "fuse-devel",
        ]),
        spurs_util::centos::yum_install(&["devtoolset-7"]),

        // Add user to libvirt group after installing
        spurs_util::util::add_to_group("libvirt"),
    }

    let installed = ushell
        .run(cmd!("yum list installed vagrant | grep -q vagrant"))
        .is_ok();

    if !installed {
        ushell.run(cmd!("sudo yum -y install {}", VAGRANT_RPM_URL))?;
    }

    let installed = ushell
        .run(cmd!("vagrant plugin list | grep -q libvirt"))
        .is_ok();

    if !installed {
        ushell.run(cmd!("vagrant plugin install vagrant-libvirt"))?;
    }

    // Need a new shell so that we get the new user group
    *ushell = SshShell::with_default_key(cfg.login.username.as_str(), &cfg.login.host)?;
    ushell.set_dry_run(cfg.dry_run);

    if let Some(device) = cfg.device {
        // Set up home device/directory
        // - format the device and create a partition
        // - mkfs on the partition
        // - copy data to new partition and mount as home dir
        ushell.run(spurs_util::util::write_gpt(device))?;
        ushell.run(spurs_util::util::create_partition(device))?;
        spurs_util::util::format_partition_as_ext4(
            ushell,
            cfg.dry_run,
            &format!("{}1", device), // assume it is the first device partition
            user_home,
            cfg.login.username.as_str(),
        )?;
    }

    // Setup swap devices, and leave a research-settings.json file. If no swap devices were
    // specififed, use all unpartitioned, unmounted devices.
    if let Some(mapper_device) = cfg.mapper_device {
        // Setup a thinkly provisioned swap device

        const DM_META_FILE: &str = "dm.meta";

        // create a 1GB zeroed file to be mounted as a loopback device for use as metadata dev for thin pool
        ushell.run(cmd!("sudo fallocate -z -l 1073741824 {}", DM_META_FILE))?;

        create_thin_swap(&ushell, DM_META_FILE, mapper_device)?;

        // Save so that we can mount on reboot.
        crate::common::set_remote_research_setting(&ushell, "dm-meta", DM_META_FILE)?;
        crate::common::set_remote_research_setting(&ushell, "dm-data", mapper_device)?;
    } else if cfg.swap_devs.is_empty() {
        let unpartitioned = spurs_util::util::get_unpartitioned_devs(ushell, cfg.dry_run)?;
        for dev in unpartitioned.iter() {
            ushell.run(cmd!("sudo mkswap /dev/{}", dev))?;
        }
    } else {
        for dev in cfg.swap_devs.iter() {
            ushell.run(cmd!("sudo mkswap /dev/{}", dev))?;
        }

        crate::common::set_remote_research_setting(&ushell, "swap-devices", &cfg.swap_devs)?;
    }

    // clone the research workspace and build/install the 0sim kernel.
    if let Some(git_branch) = cfg.git_branch {
        const CONFIG_SET: &[(&str, bool)] = &[
            // turn on 0sim
            ("CONFIG_ZSWAP", true),
            ("CONFIG_ZPOOL", true),
            ("CONFIG_ZBUD", true),
            ("CONFIG_ZTIER", true),
            ("CONFIG_SBALLOC", true),
            ("CONFIG_ZSMALLOC", true),
            ("CONFIG_X86_TSC_OFFSET_HOST_ELAPSED", true),
            ("CONFIG_SSDSWAP", true),
            // disable spectre/meltdown mitigations
            ("CONFIG_PAGE_TABLE_ISOLATION", false),
            ("CONFIG_RETPOLINE", false),
            // for `perf` stack traces
            ("CONFIG_FRAME_POINTER", true),
        ];

        const SUBMODULES: &[&str] = &[
            ZEROSIM_KERNEL_SUBMODULE,
            ZEROSIM_EXPERIMENTS_SUBMODULE,
            ZEROSIM_TRACE_SUBMODULE,
            ZEROSIM_HIBENCH_SUBMODULE,
            ZEROSIM_MEMHOG_SUBMODULE,
            ZEROSIM_METIS_SUBMODULE,
        ];

        let kernel_path = dir!(
            user_home.as_str(),
            RESEARCH_WORKSPACE_PATH,
            ZEROSIM_KERNEL_SUBMODULE
        );

        let git_hash = crate::common::clone_research_workspace(&ushell, cfg.token, SUBMODULES)?;

        crate::common::build_kernel(
            cfg.dry_run,
            &ushell,
            KernelSrc::Git {
                repo_path: kernel_path.clone(),
                git_branch: git_branch.into(),
            },
            KernelConfig {
                base_config: KernelBaseConfigSource::Current,
                extra_options: CONFIG_SET,
            },
            Some(&crate::common::gen_local_version(git_branch, &git_hash)),
            KernelPkgType::Rpm,
        )?;

        // Get name of RPM by looking for most recent file.
        let kernel_rpm = ushell
            .run(
                cmd!(
                    "basename `ls -Art {}/rpmbuild/RPMS/x86_64/ | grep -v headers | tail -n 1`",
                    user_home
                )
                .use_bash(),
            )?
            .stdout;
        let kernel_rpm = kernel_rpm.trim();

        ushell.run(
            cmd!(
                "sudo rpm -ivh --force {}/rpmbuild/RPMS/x86_64/{}",
                user_home,
                kernel_rpm
            )
            .use_bash(),
        )?;

        // Build cpupower
        ushell.run(
            cmd!("make")
                .cwd(&format!("{}/tools/power/cpupower/", kernel_path))
                .dry_run(cfg.dry_run),
        )?;

        // disable Intel EPT if needed
        if cfg.disable_ept {
            ushell.run(
                cmd!(
                    r#"echo "options kvm-intel ept=0" | \
                           sudo tee /etc/modprobe.d/kvm-intel.conf"#
                )
                .use_bash(),
            )?;

            ushell.run(cmd!("sudo rmmod kvm_intel"))?;
            ushell.run(cmd!("sudo modprobe kvm_intel"))?;

            ushell.run(cmd!("sudo tail /sys/module/kvm_intel/parameters/ept"))?;
        }

        // Build and Install QEMU 4.0.0 from source
        ushell.run(cmd!("wget {}", QEMU_TARBALL))?;
        ushell.run(cmd!("tar xvf {}", QEMU_TARBALL_NAME))?;

        let qemu_dir = QEMU_TARBALL_NAME.trim_end_matches(".tar.xz");
        let ncores = crate::common::get_num_cores(&ushell)?;

        with_shell! { ushell in qemu_dir =>
            cmd!("./configure"),
            cmd!("make -j {}", ncores),
            cmd!("sudo make install"),
        }

        ushell.run(cmd!(
            "sudo chown qemu:kvm /usr/local/bin/qemu-system-x86_64"
        ))?;

        // Make sure libvirtd can run the qemu binary
        ushell.run(cmd!(
            r#"sudo sed -i 's/#security_driver = "selinux"/security_driver = "none"/' \
                        /etc/libvirt/qemu.conf"#
        ))?;

        // Make sure libvirtd can access kvm
        ushell.run(cmd!(
            r#"echo 'KERNEL=="kvm", GROUP="kvm", MODE="0666"' |\
                               sudo tee /lib/udev/rules.d/99-kvm.rules"#
        ))?;

        crate::common::service(&ushell, "libvirtd", ServiceAction::Restart)?;

        // update grub to choose this entry (new kernel) by default
        ushell.run(cmd!("sudo grub2-set-default 0"))?;
    }

    ushell.run(cmd!("mkdir -p {}", HOSTNAME_SHARED_RESULTS_DIR))?;

    // change image location
    ushell.run(cmd!("mkdir -p images/"))?;
    ushell.run(cmd!("chmod +x ."))?;
    ushell.run(cmd!("chmod +x images/"))?;
    ushell.run(cmd!(
        "sudo chown {}:qemu images/",
        cfg.login.username.as_str()
    ))?;

    crate::common::service(&ushell, "libvirtd", ServiceAction::Start)?;

    let def_exists = ushell
        .run(cmd!("sudo virsh pool-list --all | grep -q default"))
        .is_ok();
    if def_exists {
        ushell.run(cmd!("sudo virsh pool-destroy default"))?;
        ushell.run(cmd!("sudo virsh pool-undefine default"))?;
    }

    ushell.run(cmd!(
        "sudo virsh pool-define-as --name default --type dir --target {}/images/",
        user_home
    ))?;
    ushell.run(cmd!("sudo virsh pool-autostart default"))?;
    ushell.run(cmd!("sudo virsh pool-start default"))?;
    ushell.run(cmd!("sudo virsh pool-list"))?;

    spurs::util::reboot(ushell, cfg.dry_run)?;

    Ok(())
}

/// Create the VM and install dependencies for the benchmarks/simulator. Returns root and user
/// shells to the VM.
fn init_vm<A>(
    ushell: &mut SshShell,
    cfg: &SetupConfig<'_, A>,
) -> Result<(SshShell, SshShell), failure::Error>
where
    A: std::net::ToSocketAddrs + std::fmt::Display + std::fmt::Debug + Clone,
{
    // Disable TSC offsetting so that setup runs faster
    ushell.run(
        cmd!("echo 0 | sudo tee /sys/module/kvm_intel/parameters/enable_tsc_offsetting").use_bash(),
    )?;

    // Disable firewalld if it is running because it causes VM issues. When we do that, we need to
    // reastart libvirtd.
    let firewall_up = ushell.run(cmd!("sudo firewall-cmd --state")).is_ok(); // returns 252 if not running
    if firewall_up {
        ushell.run(cmd!("sudo firewall-cmd --permanent --add-service=nfs"))?;
        ushell.run(cmd!("sudo firewall-cmd --permanent --add-service=rpc-bind"))?;
        ushell.run(cmd!("sudo firewall-cmd --permanent --add-service=mountd"))?;
        ushell.run(cmd!("sudo firewall-cmd --reload"))?;
        crate::common::service(&ushell, "firewalld", ServiceAction::Disable)?;
    }

    crate::common::service(&ushell, "libvirtd", ServiceAction::Restart)?;

    // Create the VM and add our ssh key to it.
    let vagrant_path = &dir!(RESEARCH_WORKSPACE_PATH, VAGRANT_SUBDIRECTORY);

    if cfg.destroy_existing_vm {
        with_shell! { ushell in vagrant_path =>
            cmd!("vagrant halt --force || [ ! -e Vagrantfile ]").use_bash(),
            cmd!("vagrant destroy --force || [ ! -e Vagrantfile ]").use_bash(),
        }
    }

    ushell.run(cmd!("cp Vagrantfile.bk Vagrantfile").cwd(vagrant_path))?;
    crate::common::gen_new_vagrantdomain(&ushell)?;

    gen_vagrantfile(&ushell, 20, 1)?;

    ushell.run(cmd!("vagrant halt").cwd(vagrant_path))?;
    ushell.run(cmd!("vagrant up").cwd(vagrant_path))?; // This creates the VM

    let key = std::fs::read_to_string(dir!(SSH_LOCATION, "id_rsa.pub"))?;
    let key = key.trim();
    ushell.run(
        cmd!(
            "vagrant ssh -- 'echo {} >> /home/vagrant/.ssh/authorized_keys'",
            key
        )
        .cwd(vagrant_path),
    )?;
    ushell.run(cmd!("vagrant ssh -- sudo cp -r /home/vagrant/.ssh /root/").cwd(vagrant_path))?;

    // Old key will be cached for the VM, but it is likely to have changed
    let (host, _) = spurs::util::get_host_ip(&cfg.login.host);
    let _ = Command::new("ssh-keygen")
        .args(&[
            "-f",
            &dir!(SSH_LOCATION, "known_hosts"),
            "-R",
            &format!("[{}]:{}", host, VAGRANT_PORT),
        ])
        .status()
        .unwrap();

    // Start vagrant
    let mut vrshell = start_vagrant(&ushell, &cfg.login.host, 20, 1, /* fast */ true)?;
    let mut vushell = connect_to_vagrant_as_user(&cfg.login.host)?;

    // Sometimes on adsl, networking is kind of messed up until a host restart. Check for
    // connectivity, and try restarting.
    let pub_net = vushell.run(cmd!("ping -c 1 -W 10 1.1.1.1")).is_ok();
    if !pub_net {
        ushell.run(cmd!("vagrant halt").cwd(vagrant_path))?;
        spurs::util::reboot(ushell, cfg.dry_run)?;

        vrshell = start_vagrant(&ushell, &cfg.login.host, 20, 1, /* fast */ true)?;
        vushell = connect_to_vagrant_as_user(&cfg.login.host)?;
    }

    // Keep tsc offsetting off (it may be turned on by start_vagrant).
    ushell.run(
        cmd!("echo 0 | sudo tee /sys/module/kvm_intel/parameters/enable_tsc_offsetting").use_bash(),
    )?;

    // If needed, setup the proxy.
    if let Some(proxy) = cfg.setup_proxy {
        // user
        with_shell! { vushell =>
            cmd!("echo export http_proxy='{}' | tee --append .bashrc", proxy).use_bash(),
            cmd!("echo export https_proxy='{}' | tee --append .bashrc", proxy).use_bash(),
            cmd!("echo export HTTP_PROXY='{}' | tee --append .bashrc", proxy).use_bash(),
            cmd!("echo export HTTPS_PROXY='{}' | tee --append .bashrc", proxy).use_bash(),
        }

        // root
        with_shell! { vrshell =>
            cmd!("echo export http_proxy='{}' | tee --append .bashrc", proxy).use_bash(),
            cmd!("echo export https_proxy='{}' | tee --append .bashrc", proxy).use_bash(),
            cmd!("echo export HTTP_PROXY='{}' | tee --append .bashrc", proxy).use_bash(),
            cmd!("echo export HTTPS_PROXY='{}' | tee --append .bashrc", proxy).use_bash(),
        }

        // proxy
        vrshell
            .run(cmd!("echo proxy=https://{} | tee --append /etc/yum.conf", proxy).use_bash())?;

        // need to restart shell to get new env
        vrshell = connect_to_vagrant_as_root(&cfg.login.host)?;
        vushell = connect_to_vagrant_as_user(&cfg.login.host)?;
    }

    // Install stuff on the VM
    vrshell.run(spurs_util::centos::yum_install(&["epel-release"]))?;
    vrshell.run(spurs_util::centos::yum_install(&[
        "vim",
        "git",
        "memcached",
        "gcc",
        "gcc-c++",
        "libcgroup",
        "libcgroup-tools",
        "java-1.8.0-openjdk",
        "maven",
        "numactl", // for memhog
        "redis",
        "perf", // for debugging
    ]))?;

    vrshell.run(
        cmd!(
            "curl https://sh.rustup.rs -sSf | \
             sh -s -- --default-toolchain nightly --no-modify-path -y"
        )
        .use_bash()
        .no_pty(),
    )?;
    vushell.run(
        cmd!(
            "curl https://sh.rustup.rs -sSf | \
             sh -s -- --default-toolchain nightly --no-modify-path -y"
        )
        .use_bash()
        .no_pty(),
    )?;
    ushell.run(
        cmd!(
            "curl https://sh.rustup.rs -sSf | \
             sh -s -- --default-toolchain nightly --no-modify-path -y"
        )
        .use_bash()
        .no_pty(),
    )?;

    // Build 0sim trace tool
    ushell.run(
        cmd!("$HOME/.cargo/bin/cargo build --release")
            .use_bash()
            .cwd(dir!(RESEARCH_WORKSPACE_PATH, ZEROSIM_TRACE_SUBMODULE)),
    )?;

    Ok((vrshell, vushell))
}

/// Install a recent kernel on the guest.
///
/// We will compile on the host and copy the config and the RPM through the shared directory.
fn install_guest_kernel<A>(
    ushell: &SshShell,
    vrshell: &SshShell,
    vushell: &SshShell,
    cfg: &SetupConfig<'_, A>,
) -> Result<(), failure::Error>
where
    A: std::net::ToSocketAddrs + std::fmt::Display + std::fmt::Debug + Clone,
{
    let user_home = &get_user_home_dir(&ushell)?;

    let guest_config = vushell
        .run(cmd!("ls -1 /boot/config-* | head -n1").use_bash())?
        .stdout;
    let guest_config = guest_config.trim();
    vushell.run(cmd!("cp {} {}", guest_config, VAGRANT_SHARED_DIR))?;

    let guest_config_base_name = std::path::Path::new(guest_config).file_name().unwrap();

    ushell.run(cmd!("wget {}", KERNEL_RECENT_TARBALL))?;
    crate::common::build_kernel(
        cfg.dry_run,
        &ushell,
        KernelSrc::Tar {
            tarball_path: KERNEL_RECENT_TARBALL_NAME.into(),
        },
        KernelConfig {
            base_config: KernelBaseConfigSource::Path(dir!(
                HOSTNAME_SHARED_DIR,
                guest_config_base_name.to_str().unwrap()
            )),
            extra_options: &[
                // disable spectre/meltdown mitigations
                ("CONFIG_PAGE_TABLE_ISOLATION", false),
                ("CONFIG_RETPOLINE", false),
                // for `perf` stack traces
                ("CONFIG_FRAME_POINTER", true),
            ],
        },
        None,
        KernelPkgType::Rpm,
    )?;

    // Get name of RPM by looking for most recent file.
    let kernel_rpm = ushell
        .run(
            cmd!(
                "basename `ls -Art {}/rpmbuild/RPMS/x86_64/ | grep -v headers | tail -n 1`",
                user_home
            )
            .use_bash(),
        )?
        .stdout;
    let kernel_rpm = kernel_rpm.trim();

    ushell.run(
        cmd!(
            "cp {}/rpmbuild/RPMS/x86_64/{} {}/",
            user_home,
            kernel_rpm,
            dir!(user_home.as_str(), HOSTNAME_SHARED_DIR)
        )
        .use_bash(),
    )?;

    vrshell.run(cmd!(
        "rpm -ivh --force {}",
        dir!(VAGRANT_SHARED_DIR, kernel_rpm)
    ))?;

    vrshell.run(cmd!("sudo grub2-set-default 0"))?;

    Ok(())
}

fn install_benchmarks<A>(
    ushell: &SshShell,
    vushell: &SshShell,
    cfg: &SetupConfig<'_, A>,
) -> Result<(), failure::Error>
where
    A: std::net::ToSocketAddrs + std::fmt::Display + std::fmt::Debug + Clone,
{
    // 0sim-experiments
    vushell.run(
        cmd!("/home/vagrant/.cargo/bin/cargo build --release").cwd(&dir!(
            "/home/vagrant",
            RESEARCH_WORKSPACE_PATH,
            ZEROSIM_EXPERIMENTS_SUBMODULE
        )),
    )?;

    // NAS 3.4
    ushell.run(
        cmd!("tar xvf NPB3.4.tar.gz").cwd(&dir!(RESEARCH_WORKSPACE_PATH, ZEROSIM_BENCHMARKS_DIR)),
    )?;

    with_shell! { ushell
        in &dir!(RESEARCH_WORKSPACE_PATH, ZEROSIM_BENCHMARKS_DIR, "NPB3.4", "NPB3.4-OMP") =>

        cmd!("cp config/NAS.samples/make.def_gcc config/make.def"),
        cmd!(
            "sed -i 's/^FFLAGS.*$/FFLAGS  = -O3 -fopenmp \
             -m64 -fdefault-integer-8/' config/make.def"
        ),
        cmd!("(source /opt/rh/devtoolset-7/enable ; make clean cg CLASS=E )"),
    }

    // Hadoop/spark/hibench
    if cfg.setup_hadoop {
        with_shell! { vushell =>
            cmd!("ssh-keygen -t rsa -N '' -f ~/.ssh/id_rsa").no_pty(),
            cmd!("cat ~/.ssh/id_rsa.pub >> ~/.ssh/authorized_keys"),

            cmd!(
                "echo 'source {}/hadoop_env.sh' >> ~/.bashrc",
                dir!(
                    RESEARCH_WORKSPACE_PATH,
                    ZEROSIM_BENCHMARKS_DIR,
                    ZEROSIM_HADOOP_PATH
                )
            )
        }

        with_shell! { ushell
            in &dir!(RESEARCH_WORKSPACE_PATH, ZEROSIM_BENCHMARKS_DIR, ZEROSIM_HADOOP_PATH) =>

            cmd!("wget {} {}", HADOOP_TARBALL, SPARK_TARBALL),
            cmd!("tar xvzf {}", HADOOP_TARBALL_NAME),
            cmd!("tar xvzf {}", SPARK_TARBALL_NAME),
            cmd!("cp hadoop-conf/* {}/etc/hadoop/", HADOOP_HOME),
            cmd!("cp spark-conf/* {}/conf/", SPARK_HOME),
            cmd!("cp hibench-conf/* HiBench/conf/"),
        }

        vushell.run(
            cmd!("sh -x setup.sh")
                .use_bash()
                .cwd(&dir!(
                    RESEARCH_WORKSPACE_PATH,
                    ZEROSIM_BENCHMARKS_DIR,
                    ZEROSIM_HADOOP_PATH
                ))
                .use_bash(),
        )?;
    }

    // memhog
    vushell.run(cmd!("make").cwd(&dir!(RESEARCH_WORKSPACE_PATH, ZEROSIM_MEMHOG_SUBMODULE)))?;

    // Metis
    with_shell! { vushell in &dir!(RESEARCH_WORKSPACE_PATH, ZEROSIM_METIS_SUBMODULE) =>
        cmd!("./configure"),
        cmd!("make"),
    }

    // Eager paging scripts/programs
    ushell.run(cmd!("make").cwd(&dir!(
        RESEARCH_WORKSPACE_PATH,
        ZEROSIM_BENCHMARKS_DIR,
        ZEROSIM_SWAPNIL_PATH
    )))?;

    Ok(())
}
