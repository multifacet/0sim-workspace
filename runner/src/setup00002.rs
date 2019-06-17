//! Setup the given test node such that it is using the kernel compiled from the given kernel
//! branch.
//!
//! Requires `setup00000`.

use clap::clap_app;

use spurs::{
    cmd,
    ssh::{Execute, SshShell},
};

use crate::common::{
    get_user_home_dir, KernelBaseConfigSource, KernelConfig, KernelPkgType, KernelSrc, Login,
    Username, RESEARCH_WORKSPACE_PATH, ZEROSIM_EXPERIMENTS_SUBMODULE, ZEROSIM_KERNEL_SUBMODULE,
};

pub fn cli_options() -> clap::App<'static, 'static> {
    clap_app! { setup00002 =>
        (about: "Sets up the given _centos_ machine for use exp00004. Requires `sudo`.")
        (@arg HOSTNAME: +required +takes_value
         "The domain name of the remote (e.g. c240g2-031321.wisc.cloudlab.us:22)")
        (@arg USERNAME: +required +takes_value
         "The username on the remote (e.g. markm)")
        (@arg TOKEN: +required +takes_value
         "This is the Github personal token for cloning the repo.")
        (@arg GIT_BRANCH: +takes_value -g --git_branch
         "(Optional) The git branch to compile the kernel from (e.g. markm_ztier)")
    }
}

pub fn run(dry_run: bool, sub_m: &clap::ArgMatches<'_>) -> Result<(), failure::Error> {
    let login = Login {
        username: Username(sub_m.value_of("USERNAME").unwrap()),
        hostname: sub_m.value_of("HOSTNAME").unwrap(),
        host: sub_m.value_of("HOSTNAME").unwrap(),
    };
    let git_branch = sub_m.value_of("GIT_BRANCH");
    let token = sub_m.value_of("TOKEN").unwrap();

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
            KernelSrc::Git {
                repo_path: kernel_path,
                git_branch: git_branch.into(),
            },
            KernelConfig {
                base_config: KernelBaseConfigSource::Current,
                extra_options: CONFIG_SET,
            },
            Some(&crate::common::gen_local_version(git_branch, &git_hash)),
            KernelPkgType::Rpm,
        )?;

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
            "sudo rpm -ivh --force {}/rpmbuild/RPMS/x86_64/{}",
            user_home,
            kernel_rpm
        ))?;

        // update grub to choose this entry (new kernel) by default
        ushell.run(cmd!("sudo grub2-set-default 0"))?;
    }

    // Install stuff
    ushell.run(spurs_util::centos::yum_install(&[
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
