//! Setup the given cloudlab node and VM such that the guest VM is using the kernel compiled from
//! the given kernel branch.
//!
//! Requires `setup00000`.

use spurs::{cmd, ssh::Execute};

use crate::common::{KernelPkgType, Login, RESEARCH_WORKSPACE_PATH, ZEROSIM_KERNEL_SUBMODULE};

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
    let user_home = &format!("/users/{}/", login.username.as_str());
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

    crate::common::build_kernel(
        dry_run,
        &ushell,
        &kernel_path,
        git_branch,
        CONFIG_SET,
        &format!("{}-{}", git_branch.replace("_", "-"), git_hash),
        KernelPkgType::Rpm,
    )?;

    // Install on the guest. To do this, we need the guest to be up and connected to NFS, so we can
    // copy the RPM over.
    let kernel_rpm = ushell
        .run(
            cmd!("ls -t1 | head -n2 | sort | tail -n1")
                .use_bash()
                .cwd(&format!("{}/rpmbuild/RPMS/x86_64/", user_home)),
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
