//! Setup the given test node and VM such that the guest VM is using the kernel compiled from
//! the given kernel branch.
//!
//! Requires `setup00000`.

use clap::clap_app;

use spurs::{cmd, ssh::Execute};

use crate::common::{
    exp_0sim::*,
    get_user_home_dir,
    paths::{setup00000::*, setup00001::*, *},
    KernelBaseConfigSource, KernelConfig, KernelPkgType, KernelSrc, Login, Username,
};

pub const GUEST_SWAP_GBS: usize = 10;

pub fn cli_options() -> clap::App<'static, 'static> {
    clap_app! { setup00001 =>
        (about: "Sets up the given _centos_ VM for use exp00003. Requires `sudo`.")
        (@arg HOSTNAME: +required +takes_value
         "The domain name of the remote (e.g. c240g2-031321.wisc.cloudlab.us:22)")
        (@arg USERNAME: +required +takes_value
         "The username on the remote (e.g. markm)")
        (@arg GIT_BRANCH: +required +takes_value
         "The git branch to compile the kernel from (e.g. markm_ztier)")
    }
}

pub fn run(dry_run: bool, sub_m: &clap::ArgMatches<'_>) -> Result<(), failure::Error> {
    let login = Login {
        username: Username(sub_m.value_of("USERNAME").unwrap()),
        hostname: sub_m.value_of("HOSTNAME").unwrap(),
        host: sub_m.value_of("HOSTNAME").unwrap(),
    };
    let git_branch = sub_m.value_of("GIT_BRANCH").unwrap();

    // Connect to the remote.
    let (ushell, vshell) = connect_and_setup_host_and_vagrant(dry_run, &login, 20, 1)?;

    // Disable TSC offsetting so that setup runs faster
    ushell.run(
        cmd!("echo 0 | sudo tee /sys/module/kvm_intel/parameters/enable_tsc_offsetting").use_bash(),
    )?;

    // Install the instrumented kernel on the guest.
    //
    // Building the kernel on the guest is painful, so we will build it on the host and copy it to
    // the guest via NFS.
    let user_home = &get_user_home_dir(&ushell)?;
    let kernel_path = &dir!(user_home, RESEARCH_WORKSPACE_PATH, ZEROSIM_KERNEL_SUBMODULE);

    ushell.run(cmd!("git checkout {}", git_branch).cwd(kernel_path))?;

    // Make sure we are running the latest version... sigh... made this mistake enough...
    ushell.run(cmd!("git pull").cwd(kernel_path))?;

    const CONFIG_SET: &[(&str, bool)] = &[
        ("CONFIG_ZSWAP", true),
        ("CONFIG_ZPOOL", true),
        ("CONFIG_ZBUD", true),
        ("CONFIG_ZTIER", true),
        ("CONFIG_SBALLOC", true),
        ("CONFIG_ZSMALLOC", true),
        ("CONFIG_PAGE_TABLE_ISOLATION", false),
        ("CONFIG_RETPOLINE", false),
        ("CONFIG_FRAME_POINTER", true),
    ];

    let git_hash = ushell.run(cmd!("git rev-parse HEAD").cwd(RESEARCH_WORKSPACE_PATH))?;
    let git_hash = git_hash.stdout.trim();

    let guest_config = vshell
        .run(cmd!("ls -1 /boot/config-* | head -n1").use_bash())?
        .stdout;
    let guest_config = guest_config.trim();
    vshell.run(cmd!("cp {} {}", guest_config, VAGRANT_SHARED_DIR))?;

    let guest_config_base_name = std::path::Path::new(guest_config).file_name().unwrap();

    crate::common::build_kernel(
        dry_run,
        &ushell,
        KernelSrc::Git {
            repo_path: kernel_path.clone(),
            git_branch: git_branch.into(),
        },
        KernelConfig {
            base_config: KernelBaseConfigSource::Path(dir!(
                HOSTNAME_SHARED_DIR,
                guest_config_base_name.to_str().unwrap()
            )),
            extra_options: CONFIG_SET,
        },
        Some(&crate::common::gen_local_version(git_branch, git_hash)),
        KernelPkgType::Rpm,
    )?;

    // Install on the guest. To do this, we need the guest to be up and connected to NFS, so we can
    // copy the RPM over.
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

    ushell.run(cmd!(
        "cp {}/rpmbuild/RPMS/x86_64/{} {}/vm_shared/",
        user_home,
        kernel_rpm,
        user_home
    ))?;

    vshell.run(cmd!(
        "sudo rpm -ivh --force /vagrant/vm_shared/{}",
        kernel_rpm
    ))?;

    // create a swap device if it doesn't exist already. Note that on XFS, fallocate produces files
    // with holes, so we need to manually fill them (slow and annoying, but there isn't another
    // way, unfortunately).
    with_shell! { vshell =>
        cmd!(
            "[ -e {} ] || (fallocate -z -l {} {} && dd if=/dev/zero of={})",
            VAGRANT_GUEST_SWAPFILE,
            GUEST_SWAP_GBS << 30, /* GB */
            VAGRANT_GUEST_SWAPFILE,
            VAGRANT_GUEST_SWAPFILE
        )
        .use_bash(),
        cmd!("mkswap {}", VAGRANT_GUEST_SWAPFILE),
        cmd!("sudo chmod 0600 {}", VAGRANT_GUEST_SWAPFILE),
        cmd!("sudo chown root:root {}", VAGRANT_GUEST_SWAPFILE),
    }
    crate::common::set_remote_research_setting(&ushell, "guest_swap", VAGRANT_GUEST_SWAPFILE)?;

    // update grub to choose this entry (new kernel) by default
    vshell.run(cmd!("sudo grub2-set-default 0"))?;

    // We need to sync and shut down properly to make sure the boot section of the virtual drive is
    // not corrupted. If it is corrupted, you need to basically recreate the VM :(
    vshell.run(cmd!("sync"))?;
    ushell.run(cmd!("sync"))?;

    let _ = vshell.run(cmd!("sudo poweroff"));

    Ok(())
}
