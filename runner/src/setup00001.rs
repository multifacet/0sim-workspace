//! Setup the given test node and VM such that the guest VM is using the kernel compiled from
//! the given kernel branch.
//!
//! Requires `setup00000`.

use spurs::{cmd, ssh::Execute};

use crate::common::{
    get_user_home_dir, setup00000::HOSTNAME_SHARED_DIR, KernelBaseConfigSource, KernelConfig,
    KernelPkgType, KernelSrc, Login, RESEARCH_WORKSPACE_PATH, ZEROSIM_KERNEL_SUBMODULE,
};

pub fn run<A>(dry_run: bool, login: &Login<A>, git_branch: &str) -> Result<(), failure::Error>
where
    A: std::net::ToSocketAddrs + std::fmt::Display + std::fmt::Debug,
{
    // Connect to the remote.
    let (ushell, vshell) =
        crate::common::exp00000::connect_and_setup_host_and_vagrant(dry_run, &login, 20, 1)?;

    // Install the instrumented kernel on the guest.
    //
    // Building the kernel on the guest is painful, so we will build it on the host and copy it to
    // the guest via NFS.
    let user_home = &get_user_home_dir(&ushell)?;
    let kernel_path = &format!(
        "{}/{}/{}",
        user_home, RESEARCH_WORKSPACE_PATH, ZEROSIM_KERNEL_SUBMODULE
    );

    ushell.run(cmd!("git checkout {}", git_branch).cwd(kernel_path))?;

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
    let guest_config = guest_config.trim().into();
    vshell.run(cmd!(
        "cp {} {}",
        guest_config,
        crate::common::exp00000::VAGRANT_SHARED_DIR
    ))?;

    let guest_config_base_name = std::path::Path::new(guest_config).file_name().unwrap();

    crate::common::build_kernel(
        dry_run,
        &ushell,
        KernelSrc::Git {
            repo_path: kernel_path.clone(),
            git_branch: git_branch.into(),
        },
        KernelConfig {
            base_config: KernelBaseConfigSource::Path(format!(
                "{}/{}",
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

    // update grub to choose this entry (new kernel) by default
    vshell.run(cmd!("sudo grub2-set-default 0"))?;

    Ok(())
}
