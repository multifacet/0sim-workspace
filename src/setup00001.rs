//! Setup the given cloudlab node and VM such that the guest VM is using the kernel compiled from
//! the given kernel branch.
//!
//! Requires `setup00000`.

use std::process::Command;

use spurs::cmd;

use crate::common::Login;

pub fn run<A: std::net::ToSocketAddrs + std::fmt::Display>(
    dry_run: bool,
    login: &Login<A>,
    git_branch: &str,
) -> Result<(), failure::Error> {
    // Connect to the remote.
    let (ushell, vshell) =
        crate::common::exp00000::connect_and_setup_host_and_vagrant(dry_run, &login, 20, 1)?;

    // Install the instrumented kernel on the guest.
    //
    // Building the kernel on the guest is painful, so we will build it on the host and copy it to
    // the guest via NFS.
    ushell.run(
        cmd!("git checkout side").cwd(&format!("/users/{}/linux-dev", login.username.as_str())),
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
                    &login.host,
                    login.username.as_str()
                ),
                git_branch,
            ])
            .current_dir("/u/m/a/markm/private/large_mem/software/linux-dev")
            .status()?;
    }
    ushell.run(
        cmd!("git checkout {}", git_branch)
            .cwd(&format!("/users/{}/linux-dev", login.username.as_str())),
    )?;

    // compile linux-dev
    ushell.run(cmd!("cp .config config.bak").cwd(&format!(
        "/users/{}/linux-dev/kbuild",
        login.username.as_str()
    )))?;
    ushell.run(cmd!("yes '' | make oldconfig").cwd(&format!(
        "/users/{}/linux-dev/kbuild",
        login.username.as_str()
    )))?;

    let nprocess = ushell.run(cmd!("getconf _NPROCESSORS_ONLN"))?.stdout;
    let nprocess = nprocess.trim();
    ushell.run(
        cmd!("make -j {} binrpm-pkg LOCALVERSION=-thpcmpt", nprocess)
            .cwd(&format!(
                "/users/{}/linux-dev/kbuild",
                login.username.as_str()
            ))
            .allow_error(),
    )?;

    // Install on the guest. To do this, we need the guest to be up and connected to NFS, so we can
    // copy the RPM over.
    let kernel_rpm = ushell
        .run(
            cmd!("ls -t1 | head -n2 | sort | tail -n1")
                .use_bash()
                .cwd(&format!(
                    "/users/{}/rpmbuild/RPMS/x86_64/",
                    login.username.as_str()
                )),
        )?
        .stdout;
    let kernel_rpm = kernel_rpm.trim();
    ushell.run(cmd!(
        "cp /users/{}/rpmbuild/RPMS/x86_64/{} /users/{}/vm_shared/",
        login.username.as_str(),
        kernel_rpm,
        login.username.as_str(),
    ))?;

    vshell.run(cmd!(
        "sudo rpm -ivh --force /vagrant/vm_shared/{}",
        kernel_rpm
    ))?;

    // update grub to choose this entry (new kernel) by default
    vshell.run(cmd!("sudo grub2-set-default 0"))?;

    Ok(())
}
