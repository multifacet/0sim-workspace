//! Setup the given cloudlab node such that it is using the kernel compiled from the given kernel
//! branch.
//!
//! Requires `setup00000`.

use spurs::{
    cmd,
    ssh::{Execute, SshShell},
};

use crate::common::{
    get_user_home_dir, KernelPkgType, Login, RESEARCH_WORKSPACE_PATH,
    ZEROSIM_EXPERIMENTS_SUBMODULE, ZEROSIM_KERNEL_SUBMODULE,
};

pub fn run<A>(
    dry_run: bool,
    login: &Login<A>,
    git_branch: Option<&str>,
    token: &str,
) -> Result<(), failure::Error>
where
    A: std::net::ToSocketAddrs + std::fmt::Display + std::fmt::Debug,
{
    // Connect to the remote
    let mut ushell = SshShell::with_default_key(login.username.as_str(), &login.host)?;
    ushell.set_dry_run(dry_run);

    let user_home = &get_user_home_dir(&ushell)?;

    // clone the research workspace and build/install the 0sim kernel.
    if let Some(git_branch) = git_branch {
        const CONFIG_SET: &[(&str, bool)] = &[
            ("CONFIG_PAGE_TABLE_ISOLATION", false),
            ("CONFIG_RETPOLINE", false),
            ("CONFIG_FRAME_POINTER", true),
        ];

        let kernel_path = format!(
            "{}/{}/{}",
            user_home, RESEARCH_WORKSPACE_PATH, ZEROSIM_KERNEL_SUBMODULE
        );

        let git_hash =
            crate::common::clone_research_workspace(&ushell, token, &[ZEROSIM_KERNEL_SUBMODULE])?;

        crate::common::build_kernel(
            dry_run,
            &ushell,
            &kernel_path,
            git_branch,
            CONFIG_SET,
            &format!("{}-{}", git_branch.replace("_", "-"), git_hash),
            KernelPkgType::Rpm,
        )?;

        let kernel_rpm = ushell
            .run(
                cmd!("ls -t1 | head -n2 | sort | tail -n1")
                    .use_bash()
                    .cwd(&format!("{}/rpmbuild/RPMS/x86_64/", user_home)),
            )?
            .stdout;
        let kernel_rpm = kernel_rpm.trim();

        ushell.run(cmd!(
            "sudo rpm -ivh --force {}/rpmbuild/RPMS/x86_64/{}",
            user_home,
            kernel_rpm
        ))?;

        // update grub to choose this entry (new kernel) by default
        ushell.run(cmd!("sudo grub2-set-default 0"))?;
    }

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
    ushell.run(
        cmd!("{}/.cargo/bin/cargo build --release", user_home).cwd(&format!(
            "{}/{}/{}",
            user_home, RESEARCH_WORKSPACE_PATH, ZEROSIM_EXPERIMENTS_SUBMODULE
        )),
    )?;

    Ok(())
}
