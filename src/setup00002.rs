//! Setup the given cloudlab node such that it is using the kernel compiled from the given kernel
//! branch.
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
    let ushell = crate::common::exp00000::connect_and_setup_host_only(dry_run, &login)?;

    // Build and install the required kernel from source.
    crate::common::setup00000::build_kernel_rpm(dry_run, &ushell, login, git_branch, &[], "exp")?;

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
        "sudo rpm -ivh --force /vagrant/vm_shared/{}",
        kernel_rpm
    ))?;

    // update grub to choose this entry (new kernel) by default
    ushell.run(cmd!("sudo grub2-set-default 0"))?;

    // Install stuff
    ushell.run(spurs::centos::yum_install(&[
        "vim",
        "git",
        "memcached",
        "gcc",
        "libcgroup",
        "libcgroup-tools",
    ]))?;

    ushell.run(cmd!("curl https://sh.rustup.rs -sSf | sh -s -- --default-toolchain nightly --no-modify-path -y").use_bash().no_pty())?;

    // Install benchmarks
    ushell.run(cmd!("mkdir -p paperexp").cwd(&format!("/users/{}/", login.username.as_str())))?;
    ushell.run(cmd!("git init").cwd(&format!("/users/{}/paperexp", login.username.as_str())))?;
    ushell.run(
        cmd!("git checkout -b side")
            .cwd(&format!("/users/{}/paperexp", login.username.as_str()))
            .allow_error(), // if already exists
    )?;

    if !dry_run {
        let (host, _) = spurs::util::get_host_ip(&login.host);

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
                    crate::common::exp00000::VAGRANT_PORT
                ),
                "master",
            ])
            .current_dir("/u/m/a/markm/private/large_mem/tools/paperexp/")
            .status()?;

        let _ = Command::new("git")
            .args(&["checkout", "side"])
            .current_dir("/u/m/a/markm/private/large_mem/tools/paperexp/")
            .status()?;
    }
    ushell.run(
        cmd!("git checkout master").cwd(&format!("/users/{}/paperexp", login.username.as_str())),
    )?;

    ushell.run(
        cmd!("/root/.cargo/bin/cargo build --release")
            .cwd(&format!("/users/{}/paperexp", login.username.as_str())),
    )?;

    Ok(())
}
