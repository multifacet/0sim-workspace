//! A library of routines commonly used in experiments.
//!
//! In honor of my friend Josh:
//!
//!  _━*━___━━___━__*___━_*___┓━╭━━━━━━━━━╮
//! __*_━━___━━___━━*____━━___┗┓|::::::^---^
//! ___━━___━*━___━━____━━*___━┗|::::|｡◕‿‿◕｡|
//! ___*━__━━_*___━━___*━━___*━━╰O­-O---O--O ╯

#[macro_use]
pub mod output;

use serde::{Deserialize, Serialize};

use spurs::{
    cmd,
    ssh::{Execute, SshShell},
};

#[derive(Copy, Clone, Debug)]
pub struct Username<'u>(pub &'u str);

impl Username<'_> {
    pub fn as_str(&self) -> &str {
        self.0
    }
}

pub struct Login<'u, 'h, A: std::net::ToSocketAddrs + std::fmt::Display> {
    pub host: A,
    pub hostname: &'h str,
    pub username: Username<'u>,
}

pub enum GitHubRepo {
    #[allow(dead_code)]
    Ssh {
        /// Repo git URL (e.g. `git@github.com:mark-i-m/spurs`)
        repo: String,
    },
    Https {
        /// Repo https URL (e.g. `github.com/mark-i-m/spurs`)
        repo: String,
        /// (Username, OAuth token) for authentication, if needed
        token: Option<(String, String)>,
    },
}

impl std::fmt::Display for GitHubRepo {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            GitHubRepo::Ssh { repo } => write!(f, "{}", repo),
            GitHubRepo::Https {
                repo,
                token: Some((user, token)),
            } => write!(f, "https://{}:{}@{}", user, token, repo),
            GitHubRepo::Https { repo, .. } => write!(f, "https://{}", repo),
        }
    }
}

/// The username to clone the research workspace with.
pub const GITHUB_CLONE_USERNAME: &str = "robo-mark-i-m";

/// The github repo URL for the research workspace.
pub const RESEARCH_WORKSPACE_REPO: &str = "github.com/mark-i-m/research-workspace";

/// The path at which `clone_research_workspace` clones the workspace.
pub const RESEARCH_WORKSPACE_PATH: &str = "research-workspace";

// Path to certain submodules

/// Path to the 0sim submodule.
pub const ZEROSIM_KERNEL_SUBMODULE: &str = "0sim";

/// Path to the 0sim-experiments submodule.
pub const ZEROSIM_EXPERIMENTS_SUBMODULE: &str = "0sim-experiments";

/// Path to the 0sim-trace submodule.
pub const ZEROSIM_TRACE_SUBMODULE: &str = "0sim-trace";

/// Path to the HiBench submodule.
pub const ZEROSIM_HIBENCH_SUBMODULE: &str = "bmks/zerosim-hadoop/HiBench";

/// Path to benchmarks directory.
pub const ZEROSIM_BENCHMARKS_DIR: &str = "bmks";

/// Path to Hadoop benchmark stuff within the benchmarks dir.
pub const ZEROSIM_HADOOP_PATH: &str = "zerosim-hadoop";

/// Path to the `vagrant` subdirectory where `gen_vagrantfile` will do its work.
pub const VAGRANT_SUBDIRECTORY: &str = "vagrant";

/// Clone the research-workspace and checkout the given submodules. The given token is used as the
/// Github personal access token.
///
/// Returns the git hash of the cloned repo.
///
/// *NOTE*: This function intentionally does not take the repo URL. It should always be the above.
pub fn clone_research_workspace(
    ushell: &SshShell,
    token: &str,
    submodules: &[&str],
) -> Result<String, failure::Error> {
    // Clone the repo.
    let repo = GitHubRepo::Https {
        repo: RESEARCH_WORKSPACE_REPO.into(),
        token: Some((GITHUB_CLONE_USERNAME.into(), token.into())),
    };
    ushell.run(cmd!("git clone {}", repo))?;

    // Checkout submodules.
    for submodule in submodules {
        ushell.run(
            cmd!("git submodule update --init --recursive -- {}", submodule)
                .cwd(RESEARCH_WORKSPACE_PATH),
        )?;
    }

    // Get the sha hash.
    research_workspace_git_hash(ushell)
}

/// Get the git hash of the remote research workspace.
pub fn research_workspace_git_hash(ushell: &SshShell) -> Result<String, failure::Error> {
    let hash = ushell.run(cmd!("git rev-parse HEAD").cwd(RESEARCH_WORKSPACE_PATH))?;
    let hash = hash.stdout.trim();

    Ok(hash.into())
}

/// Get the git hash of the local research workspace, specifically the workspace from which the
/// runner is run. Returns `"dirty"` if the workspace has uncommitted changes.
pub fn local_research_workspace_git_hash() -> Result<String, failure::Error> {
    let is_dirty = std::process::Command::new("git")
        .args(&["diff", "--quiet"])
        .status()?
        .code()
        .expect("terminated by signal")
        == 1;

    if is_dirty {
        return Ok("dirty".into());
    }

    let output = std::process::Command::new("git")
        .args(&["rev-parse", "HEAD"])
        .output()?;
    let output = std::str::from_utf8(&output.stdout)?;
    let output = output.trim();
    Ok(output.into())
}

/// Get the path of the user's home directory.
pub fn get_user_home_dir(ushell: &SshShell) -> Result<String, failure::Error> {
    let user_home = ushell.run(cmd!("echo $HOME").use_bash())?;
    Ok(user_home.stdout.trim().to_owned())
}

/// There are some settings that are per-machine, rather than per-experiment (e.g. which devices to
/// turn on as swap devices). We keep these settings in a per-machine file called
/// `research-settings.json`, which is generated at the time of the setup.
///
/// This function sets the given setting or overwrites its current value.
pub fn set_remote_research_setting<V: Serialize>(
    ushell: &SshShell,
    setting: &str,
    value: V,
) -> Result<(), failure::Error> {
    // Make sure the file exists
    ushell.run(cmd!("touch research-settings.json"))?;

    // We don't care too much about efficiency, so whenever we update, we will just read,
    // deserialize, update, and reserialize.
    let mut settings = get_remote_research_settings(ushell)?;

    let serialized = serde_json::to_string(&value).expect("unable to serialize");
    settings.insert(setting.into(), serialized);

    let new_contents = serde_json::to_string(&settings).expect("unable to serialize");

    ushell.run(cmd!("echo '{}' > research-settings.json", new_contents))?;

    Ok(())
}

/// Return all research settings. The user can then use `get_remote_research_setting` to parse out
/// a single value.
pub fn get_remote_research_settings(
    ushell: &SshShell,
) -> Result<std::collections::BTreeMap<String, String>, failure::Error> {
    // Make sure the file exists
    ushell.run(cmd!("touch research-settings.json"))?;

    let file_contents = ushell.run(cmd!("cat research-settings.json"))?;
    let file_contents = file_contents.stdout.trim();

    if file_contents.is_empty() {
        Ok(std::collections::BTreeMap::new())
    } else {
        Ok(serde_json::from_str(file_contents).expect("unable to deserialize"))
    }
}

/// Returns the value of the given setting if it is set.
pub fn get_remote_research_setting<'s, 'd, V: Deserialize<'d>>(
    settings: &'s std::collections::BTreeMap<String, String>,
    setting: &str,
) -> Result<Option<V>, failure::Error>
where
    's: 'd,
{
    if let Some(setting) = settings.get(setting) {
        Ok(Some(
            serde_json::from_str(setting).expect("unable to deserialize"),
        ))
    } else {
        Ok(None)
    }
}

/// Generate a new vagrant domain name and update the Vagrantfile.
pub fn gen_new_vagrantdomain(shell: &SshShell) -> Result<(), failure::Error> {
    let vagrant_path = &format!("{}/{}", RESEARCH_WORKSPACE_PATH, VAGRANT_SUBDIRECTORY);
    let uniq = shell.run(cmd!("date | sha256sum | head -c 10"))?;
    let uniq = uniq.stdout.trim();
    shell.run(cmd!("sed -i 's/:test_vm/:test_vm_{}/' Vagrantfile", uniq).cwd(vagrant_path))?;
    Ok(())
}

/// What type of package to produce from the kernel build?
pub enum KernelPkgType {
    /// `bindeb-pkg`
    #[allow(dead_code)]
    Deb,
    /// `binrpm-pkg`
    Rpm,
}

/// Where to build the kernel from?
pub enum KernelSrc {
    /// The given git repo and branch.
    ///
    /// The repo should already be cloned at the give path. This function will checkout the given
    /// branch, though, so the repo should be clean.
    Git {
        repo_path: String,
        git_branch: String,
    },

    /// The given tarball, which will be untarred and built as is. We assume that the name of the
    /// unpacked source directory is the same as the tarball name without the `.tar.gz` or `.tgz`
    /// extension.
    #[allow(dead_code)]
    Tar { tarball_path: String },
}

/// Where to get the base config (on top of which we will apply additional changes)?
pub enum KernelBaseConfigSource {
    /// Use `make defconfig`
    #[allow(dead_code)]
    Defconfig,

    /// Use the running kernel.
    Current,

    /// Use the config from the given path.
    #[allow(dead_code)]
    Path(String),
}

/// How to configure the kernel build? The config is created by taking some "base config", such as
/// the one for the running kernel, and applying some changes to it to enable or disable additional
/// options.
pub struct KernelConfig<'a> {
    pub base_config: KernelBaseConfigSource,

    /// A list of config option names that should be set or unset before building. It is the
    /// caller's responsibility to make sure that all dependencies are on too. If a config is
    /// `true` it is set to "y"; otherwise, it is unset.
    pub extra_options: &'a [(&'a str, bool)],
}

/// Build a Linux kernel package (RPM or DEB). This command does not install the new kernel.
///
/// `kernel_local_version` is the kernel `LOCALVERSION` string to pass to `make` for the RPM, if
/// any.
pub fn build_kernel(
    _dry_run: bool,
    ushell: &SshShell,
    source: KernelSrc,
    config: KernelConfig<'_>,
    kernel_local_version: Option<&str>,
    pkg_type: KernelPkgType,
) -> Result<(), failure::Error> {
    // Check out or unpack the source code, returning its path.
    let source_path = match source {
        KernelSrc::Git {
            repo_path,
            git_branch,
        } => {
            ushell.run(cmd!("git checkout {}", git_branch).cwd(&repo_path))?;

            repo_path
        }

        KernelSrc::Tar { tarball_path } => {
            ushell.run(cmd!("tar xvf {}", tarball_path))?;

            tarball_path
                .trim_end_matches(".tar.gz")
                .trim_end_matches(".tgz")
                .into()
        }
    };

    // kbuild path.
    let kbuild_path = &format!("{}/kbuild", source_path);

    ushell.run(cmd!("mkdir -p {}", kbuild_path))?;

    // save old config if there is one.
    ushell.run(cmd!("cp .config config.bak").cwd(kbuild_path).allow_error())?;

    // configure the new kernel we are about to build.
    ushell.run(cmd!("make O={} defconfig", kbuild_path).cwd(source_path))?;

    match config.base_config {
        // Nothing else to do
        KernelBaseConfigSource::Defconfig => {}

        KernelBaseConfigSource::Current => {
            let config = ushell
                .run(cmd!("ls -1 /boot/config-* | head -n1").use_bash())?
                .stdout;
            let config = config.trim();
            ushell.run(cmd!("cp {} {}/.config", config, kbuild_path))?;
            ushell.run(cmd!("yes '' | make oldconfig").use_bash().cwd(kbuild_path))?;
        }

        KernelBaseConfigSource::Path(template_path) => {
            ushell.run(cmd!("cp {} {}/.config", template_path, kbuild_path))?;
            ushell.run(cmd!("yes '' | make oldconfig").use_bash().cwd(kbuild_path))?;
        }
    }

    for (opt, set) in config.extra_options.iter() {
        if *set {
            ushell.run(cmd!(
                "sed -i 's/# {} is not set/{}=y/' {}/.config",
                opt,
                opt,
                kbuild_path
            ))?;
        } else {
            ushell.run(cmd!(
                "sed -i 's/{}=y/# {} is not set/' {}/.config",
                opt,
                opt,
                kbuild_path
            ))?;
        }
    }

    // Compile with as many processors as we have.
    //
    // NOTE: for some reason, this sometimes fails the first time, so just do it again.
    let nprocess = ushell.run(cmd!("getconf _NPROCESSORS_ONLN"))?.stdout;
    let nprocess = nprocess.trim();

    let make_target = match pkg_type {
        KernelPkgType::Deb => "bindeb-pkg",
        KernelPkgType::Rpm => "binrpm-pkg",
    };

    ushell.run(
        cmd!(
            "make -j {} {} {}",
            nprocess,
            make_target,
            if let Some(kernel_local_version) = kernel_local_version {
                format!("LOCALVERSION=-{}", kernel_local_version)
            } else {
                "".into()
            }
        )
        .cwd(kbuild_path)
        .allow_error(),
    )?;
    ushell.run(
        cmd!(
            "make -j {} {} {}",
            nprocess,
            make_target,
            if let Some(kernel_local_version) = kernel_local_version {
                format!("LOCALVERSION=-{}", kernel_local_version)
            } else {
                "".into()
            }
        )
        .cwd(kbuild_path),
    )?;

    Ok(())
}

pub mod setup00000 {
    pub const HOSTNAME_SHARED_DIR: &str = "vm_shared/";
    pub const HOSTNAME_SHARED_RESULTS_DIR: &str = "vm_shared/results/";
}

pub mod exp00000 {
    use std::collections::HashMap;

    use spurs::{
        cmd,
        ssh::{Execute, SshShell},
    };

    pub use super::{
        Login, Username, RESEARCH_WORKSPACE_PATH, VAGRANT_SUBDIRECTORY, ZEROSIM_KERNEL_SUBMODULE,
    };

    /// The port that vagrant VMs forward from.
    pub const VAGRANT_PORT: u16 = 5555;

    /// The default amount of memory of the VM.
    pub const VAGRANT_MEM: usize = 1024;

    /// The default number of cores of the VM.
    pub const VAGRANT_CORES: usize = 1;

    /// The shared directory for results.
    pub const VAGRANT_RESULTS_DIR: &str = "/vagrant/vm_shared/results/";

    /// Reboot the machine and do nothing else. Useful for getting the machine into a clean state.
    pub fn initial_reboot<A>(dry_run: bool, login: &Login<A>) -> Result<(), failure::Error>
    where
        A: std::net::ToSocketAddrs + std::fmt::Display + std::fmt::Debug,
    {
        // Connect to the remote
        let mut ushell = SshShell::with_default_key(login.username.as_str(), &login.host)?;
        ushell.set_dry_run(dry_run);

        // Reboot the remote to make sure we have a clean slate
        spurs::util::reboot(&mut ushell, dry_run)?;

        Ok(())
    }

    /// Connects to the host and to vagrant. Returns shells for both.
    pub fn connect_and_setup_host_and_vagrant<A>(
        dry_run: bool,
        login: &Login<A>,
        vm_size: usize,
        cores: usize,
    ) -> Result<(SshShell, SshShell), failure::Error>
    where
        A: std::net::ToSocketAddrs + std::fmt::Display + std::fmt::Debug,
    {
        let ushell = connect_and_setup_host_only(dry_run, &login)?;
        let vshell = start_vagrant(&ushell, &login.host, vm_size, cores)?;

        Ok((ushell, vshell))
    }

    /// Turn off all previous swap spaces, and turn on the configured ones (e.g. via
    /// research-settings.json).
    pub fn setup_swapping(shell: &SshShell, dry_run: bool) -> Result<(), failure::Error> {
        turn_off_swapdevs(shell, dry_run)?;
        turn_on_swapdevs(shell, dry_run)?;
        Ok(())
    }

    /// Set the scaling governor to "performance".
    pub fn set_perf_scaling_gov(shell: &SshShell, dry_run: bool) -> Result<(), failure::Error> {
        let user_home = crate::common::get_user_home_dir(shell)?;

        let kernel_path = format!(
            "{}/{}/{}",
            user_home, RESEARCH_WORKSPACE_PATH, ZEROSIM_KERNEL_SUBMODULE
        );

        shell.run(
            cmd!(
                "sudo {}/tools/power/cpupower/cpupower frequency-set -g performance",
                kernel_path
            )
            .dry_run(dry_run),
        )?;

        Ok(())
    }

    /// Set the kernel `printk` level that gets logged to `dmesg`. `0` is only high-priority
    /// messages. `7` is all messages.
    pub fn set_kernel_printk_level(shell: &SshShell, level: usize) -> Result<(), failure::Error> {
        assert!(level <= 7);
        shell.run(cmd!("echo {} | sudo tee /proc/sys/kernel/printk", level).use_bash())?;
        Ok(())
    }

    /// Connects to the host, waiting for it to come up if necessary. Turn on only the swap devices we
    /// want. Set the scaling governor. Returns the shell to the host.
    pub fn connect_and_setup_host_only<A>(
        dry_run: bool,
        login: &Login<A>,
    ) -> Result<SshShell, failure::Error>
    where
        A: std::net::ToSocketAddrs + std::fmt::Debug + std::fmt::Display,
    {
        // Keep trying to connect until we succeed
        let ushell = {
            let mut shell;
            loop {
                shell = match SshShell::with_default_key(login.username.as_str(), &login.host) {
                    Ok(shell) => shell,
                    Err(_) => {
                        std::thread::sleep(std::time::Duration::from_secs(10));
                        continue;
                    }
                };
                match shell.run(cmd!("whoami")) {
                    Ok(_) => break,
                    Err(_) => {
                        std::thread::sleep(std::time::Duration::from_secs(10));
                        continue;
                    }
                }
            }

            shell
        };

        ushell.run(cmd!("uname -a").dry_run(dry_run))?;

        // Set up swapping
        setup_swapping(&ushell, dry_run)?;

        println!("Assuming home dir already mounted... uncomment this line if it's not");
        //mount_home_dir(ushell)

        set_perf_scaling_gov(&ushell, dry_run)?;

        set_kernel_printk_level(&ushell, 4)?;

        Ok(ushell)
    }

    /// Turn on Zswap with some default parameters.
    pub fn turn_on_zswap(shell: &mut SshShell, dry_run: bool) -> Result<(), failure::Error> {
        shell.set_dry_run(dry_run);

        // apparently permissions can get weird
        shell.run(cmd!("sudo chmod +w /sys/module/zswap/parameters/*").use_bash())?;

        // THP is buggy with frontswap until later kernels
        shell.run(
            cmd!("echo never | sudo tee /sys/kernel/mm/transparent_hugepage/enabled").use_bash(),
        )?;

        // KSM is also not working right
        shell.run(cmd!("sudo systemctl disable ksm"))?;
        shell.run(cmd!("sudo systemctl disable ksmtuned"))?;

        shell.run(cmd!("echo ztier | sudo tee /sys/module/zswap/parameters/zpool").use_bash())?;
        shell.run(cmd!("echo y | sudo tee /sys/module/zswap/parameters/enabled").use_bash())?;
        shell.run(cmd!("sudo tail /sys/module/zswap/parameters/*").use_bash())?;

        shell.set_dry_run(false);

        Ok(())
    }

    pub fn connect_to_vagrant<A: std::net::ToSocketAddrs + std::fmt::Display>(
        hostname: A,
    ) -> Result<SshShell, failure::Error> {
        let (host, _) = spurs::util::get_host_ip(hostname);
        SshShell::with_default_key("root", (host, VAGRANT_PORT))
    }

    pub fn connect_to_vagrant_user<A: std::net::ToSocketAddrs + std::fmt::Display>(
        hostname: A,
    ) -> Result<SshShell, failure::Error> {
        let (host, _) = spurs::util::get_host_ip(hostname);
        SshShell::with_default_key("vagrant", (host, VAGRANT_PORT))
    }

    pub fn start_vagrant<A: std::net::ToSocketAddrs + std::fmt::Display>(
        shell: &SshShell,
        hostname: A,
        memgb: usize,
        cores: usize,
    ) -> Result<SshShell, failure::Error> {
        shell.run(cmd!("sudo systemctl stop firewalld"))?;
        shell.run(cmd!("sudo systemctl stop nfs-idmap.service"))?;
        shell.run(cmd!("sudo systemctl start nfs-idmap.service"))?;
        shell.run(cmd!("sudo service libvirtd restart"))?;

        // Disable KSM because it creates a lot of overhead when the host is oversubscribed
        shell.run(cmd!("sudo systemctl stop ksmtuned"))?;
        shell.run(cmd!("sudo systemctl stop ksm"))?;

        gen_vagrantfile(shell, memgb, cores)?;

        let vagrant_path = &format!("{}/{}", RESEARCH_WORKSPACE_PATH, VAGRANT_SUBDIRECTORY);

        shell.run(cmd!("vagrant halt").cwd(vagrant_path))?;

        // We want to pin the vCPUs as soon as possible because otherwise, they tend to switch
        // around a lot, causing a lot of printk overhead.
        let pin = {
            let mut pin = HashMap::new();
            for c in 0..cores {
                pin.insert(c, c);
            }
            pin
        };
        virsh_vcpupin(shell, &pin)?;

        shell.run(cmd!("vagrant up").no_pty().cwd(vagrant_path))?;

        shell.run(cmd!("sudo lsof -i -P -n | grep LISTEN").use_bash())?;
        let vshell = connect_to_vagrant(hostname)?;

        Ok(vshell)
    }

    pub fn turn_off_swapdevs(shell: &SshShell, dry_run: bool) -> Result<(), failure::Error> {
        let devs = spurs::util::get_mounted_devs(shell, dry_run)?;

        // Turn off all swap devs
        for (dev, mount) in devs {
            if mount == "[SWAP]" {
                shell.run(cmd!("sudo swapoff /dev/{}", dev))?;
            }
        }

        Ok(())
    }

    /// Returns a list of swap devices, with SSDs listed first.
    pub fn list_swapdevs(shell: &SshShell, dry_run: bool) -> Result<Vec<String>, failure::Error> {
        let mut swapdevs = vec![];

        // Find out what swap devs are there
        let devs = spurs::util::get_unpartitioned_devs(shell, dry_run)?;

        // Get the size of each one
        let sizes =
            spurs::util::get_dev_sizes(shell, devs.iter().map(String::as_str).collect(), dry_run)?;

        // Turn on the SSDs as swap devs
        for (dev, size) in devs.iter().zip(sizes.iter()) {
            if size == "447.1G" {
                swapdevs.push(dev.clone());
            }
        }

        // Turn on the HDDs as swap devs
        for (dev, size) in devs.iter().zip(sizes.iter()) {
            if ["1.1T", "2.7T", "3.7T", "931.5G"].iter().any(|s| s == size) {
                swapdevs.push(dev.clone());
            }
        }

        Ok(swapdevs)
    }

    /// Create and mount a thinly-partitioned swap device using device mapper. Device mapper
    /// requires two devices: a metadata volume and a data volume. We use a file mounted as a
    /// loopback device for the metadata volume and another arbitrary device as the data volume.
    ///
    /// The metadata volume only needs to be a few megabytes large (e.g. 1GB would be overkill).
    /// The data volume should be as large and fast as needed.
    ///
    /// This is idempotent.
    fn create_and_turn_on_thin_swap_inner(
        shell: &SshShell,
        meta_file: &str,
        data_dev: &str,
        new: bool,
    ) -> Result<(), failure::Error> {
        // Check if thin device is already created.
        let already = shell
            .run(cmd!("sudo dmsetup ls"))?
            .stdout
            .contains("mythin");

        if !already {
            // create loopback
            shell.run(cmd!("sudo losetup -f {}", meta_file))?;

            // find out which loopback device was created
            let out = shell.run(cmd!("sudo losetup -j {}", meta_file))?.stdout;
            let loopback = out.trim().split(":").next().expect("expected device name");

            // find out the size of the mapper_device
            let out = shell
                .run(cmd!("lsblk -o SIZE -b {} | sed '2q;d'", data_dev).use_bash())?
                .stdout;
            let mapper_device_size = out.trim().parse::<u64>().unwrap() >> 9; // 512B sectors

            // create a thin pool
            // - 0 is the start sector
            // - `mapper_device_size` is the end sector of the pool. This should be the size of the data device.
            // - `loopback` is the metadata device
            // - `mapper_device` is the data device
            // - 256000 = 128MB is the block size
            // - 0 indicates no dm event on low-watermark
            shell.run(cmd!(
                "sudo dmsetup create mypool --table \
                 '0 {} thin-pool {} {} 256000 0'",
                mapper_device_size,
                loopback,
                data_dev,
            ))?;

            if new {
                // create a thin volume
                // - /dev/mapper/mypool is the name of the pool device above
                // - 0 is the sector number on the pool
                // - create_thin indicates the pool should create a new thin volume
                // - 0 is a unique 24-bit volume id
                shell.run(cmd!(
                    "sudo dmsetup message /dev/mapper/mypool 0 'create_thin 0'"
                ))?;
            }

            // init the volume
            // - 0 is the start sector
            // - 21474836480 = 10TB is the end sector
            // - thin is the device type
            // - /dev/mapper/mypool is the pool to use
            // - 0 is the volume id from above
            shell.run(cmd!(
                "sudo dmsetup create mythin --table '0 21474836480 thin /dev/mapper/mypool 0'"
            ))?;

            shell.run(cmd!("sudo mkswap /dev/mapper/mythin"))?;
        }

        shell.run(cmd!("sudo swapon /dev/mapper/mythin"))?;

        Ok(())
    }

    /// Create and mount a thinly-partitioned swap device using device mapper. Device mapper
    /// requires two devices: a metadata volume and a data volume. We use a file mounted as a
    /// loopback device for the metadata volume and another arbitrary device as the data volume.
    ///
    /// The metadata volume only needs to be a few megabytes large (e.g. 1GB would be overkill).
    /// The data volume should be as large and fast as needed.
    pub fn turn_on_thin_swap(
        shell: &SshShell,
        meta_file: &str,
        data_dev: &str,
    ) -> Result<(), failure::Error> {
        create_and_turn_on_thin_swap_inner(shell, meta_file, data_dev, false)
    }

    /// Create a new thinly-partitioned swap device using device mapper. Device mapper
    /// requires two devices: a metadata volume and a data volume. We use a file mounted as a
    /// loopback device for the metadata volume and another arbitrary device as the data volume.
    ///
    /// The metadata volume only needs to be a few megabytes large (e.g. 1GB would be overkill).
    /// The data volume should be as large and fast as needed.
    pub fn create_thin_swap(
        shell: &SshShell,
        meta_file: &str,
        data_dev: &str,
    ) -> Result<(), failure::Error> {
        create_and_turn_on_thin_swap_inner(shell, meta_file, data_dev, true)
    }

    /// Turn on swap devices. This function will respect any `swap-devices` setting in
    /// `research-settings.json`. If there are no such settings, then all unpartitioned, unmounted
    /// swap devices of the right size are used (according to `list_swapdevs`).
    pub fn turn_on_swapdevs(shell: &SshShell, dry_run: bool) -> Result<(), failure::Error> {
        // Find out what swap devs are there
        let settings = crate::common::get_remote_research_settings(shell)?;

        if let (Some(dm_meta), Some(dm_data)) = (
            crate::common::get_remote_research_setting(&settings, "dm-meta")?,
            crate::common::get_remote_research_setting(&settings, "dm-data")?,
        ) {
            // If a thinly-provisioned swap space is setup, load and mount it.
            return turn_on_thin_swap(shell, dm_meta, dm_data);
        }

        let devs = if let Some(devs) =
            crate::common::get_remote_research_setting(&settings, "swap-devices")?
        {
            devs
        } else {
            list_swapdevs(shell, dry_run)?
        };

        // Turn on swap devs
        for dev in &devs {
            shell.run(cmd!("sudo swapon /dev/{}", dev))?;
        }

        shell.run(cmd!("lsblk"))?;

        Ok(())
    }

    /// Turn on swap devices and SSDSWAP. This function will respect any `swap-devices` setting in
    /// `research-settings.json`. If there are no such settings, then all unpartitioned, unmounted
    /// swap devices of the right size are used (according to `list_swapdevs`).
    pub fn turn_on_ssdswap(shell: &SshShell, dry_run: bool) -> Result<(), failure::Error> {
        // Find out what swap devs are there
        let devs = if let Some(devs) = {
            let settings = crate::common::get_remote_research_settings(shell)?;
            crate::common::get_remote_research_setting(&settings, "swap-devices")?
        } {
            devs
        } else {
            list_swapdevs(shell, dry_run)?
        };

        // Use SSDSWAP
        for dev in &devs {
            shell.run(
                cmd!(
                    "echo /dev/{} | sudo tee /sys/module/ssdswap/parameters/device",
                    dev
                )
                .use_bash(),
            )?;
        }

        // Remount all swap devs
        turn_off_swapdevs(shell, dry_run)?;
        turn_on_swapdevs(shell, dry_run)?;

        shell.run(cmd!("lsblk -o NAME,ROTA"))?;

        Ok(())
    }

    /// Get the VM domain name from `virsh` for the first running VM if there is a VM running or
    /// the first stopped VM if no VM is running. The `bool` returned indicates whether the VM is
    /// running or not (`true` is running).
    pub fn virsh_domain_name(shell: &SshShell) -> Result<(String, bool), failure::Error> {
        let running: String = shell
            .run(cmd!(
                "sudo virsh list | tail -n 2 | head -n1 | awk '{{print $2}}'"
            ))?
            .stdout
            .trim()
            .into();

        if running.is_empty() {
            Ok((
                shell
                    .run(cmd!(
                        "sudo virsh list --all | tail -n 2 | head -n1 | awk '{{print $2}}'"
                    ))?
                    .stdout
                    .trim()
                    .into(),
                false,
            ))
        } else {
            Ok((running, true))
        }
    }

    /// For `(v, p)` in `mapping`, pin vcpu `v` to host cpu `p`. `running` indicates whether the VM
    /// is running or not.
    pub fn virsh_vcpupin(
        shell: &SshShell,
        mapping: &HashMap<usize, usize>,
    ) -> Result<(), failure::Error> {
        let (domain, running) = virsh_domain_name(shell)?;

        // We may have just changed the number of vcpus in the vagrant config, so we need to make
        // sure that libvirt is up to date.
        shell.run(cmd!(
            "sudo virsh setvcpus {} {} --maximum --config",
            domain,
            mapping.len(),
        ))?;

        shell.run(cmd!("sudo virsh vcpuinfo {}", domain))?;

        for (v, p) in mapping {
            shell.run(cmd!(
                "sudo virsh vcpupin {} {} {} {}",
                domain,
                if running { "" } else { "--config" },
                v,
                p
            ))?;
        }

        shell.run(cmd!("sudo virsh vcpupin {}", domain))?;

        Ok(())
    }

    /// Generate a Vagrantfile for a VM with the given amount of memory and number of cores. A
    /// Vagrantfile should already exist containing the correct domain name.
    pub fn gen_vagrantfile(
        shell: &SshShell,
        memgb: usize,
        cores: usize,
    ) -> Result<(), failure::Error> {
        let vagrant_path = &format!("{}/{}", RESEARCH_WORKSPACE_PATH, VAGRANT_SUBDIRECTORY);

        // Keep the same VM domain name though...
        let current_name =
            shell.run(cmd!("grep -oE ':test_vm[0-9a-zA-Z_]+' Vagrantfile").cwd(vagrant_path))?;
        let current_name = current_name.stdout.trim();

        shell.run(cmd!("cp Vagrantfile.bk Vagrantfile").cwd(vagrant_path))?;
        shell.run(cmd!("sed -i 's/:test_vm/{}/' Vagrantfile", current_name).cwd(vagrant_path))?;
        shell.run(
            cmd!("sed -i 's/memory = 1023/memory = {}/' Vagrantfile", memgb).cwd(vagrant_path),
        )?;
        shell.run(cmd!("sed -i 's/cpus = 1/cpus = {}/' Vagrantfile", cores).cwd(vagrant_path))?;

        let user_home = crate::common::get_user_home_dir(shell)?;
        let vagrant_full_path = &format!("{}/{}", user_home, vagrant_path).replace("/", r#"\/"#);
        let vm_shared_full_path = &format!(
            "{}/{}",
            user_home,
            crate::common::setup00000::HOSTNAME_SHARED_DIR
        )
        .replace("/", r#"\/"#);
        let research_workspace_full_path =
            &format!("{}/{}", user_home, RESEARCH_WORKSPACE_PATH).replace("/", r#"\/"#);

        shell.run(
            cmd!(
                r#"sed -i 's/vagrant_dir = ''/vagrant_dir = "{}"/' Vagrantfile"#,
                vagrant_full_path
            )
            .cwd(vagrant_path),
        )?;
        shell.run(
            cmd!(
                r#"sed -i 's/vm_shared_dir = ''/vm_shared_dir = "{}"/' Vagrantfile"#,
                vm_shared_full_path
            )
            .cwd(vagrant_path),
        )?;
        shell.run(
            cmd!(
                r#"sed -i 's/research_workspace_dir = ''/research_workspace_dir = "{}"/' Vagrantfile"#,
                research_workspace_full_path
            )
            .cwd(vagrant_path),
        )?;

        // Choose the interface that actually gives network access. We do this by looking for the
        // interface that gives a route 1.1.1.1 (Cloudflare DNS).
        let iface = shell.run(
            cmd!(
                r#"/usr/sbin/ip route get 1.1.1.1 |\
                         grep -oE 'dev [a-z0-9]+ ' |\
                         awk '{{print $2}}'"#
            )
            .use_bash(),
        )?;
        let iface = iface.stdout.trim();

        shell.run(
            cmd!(
                r#"sed -i 's/iface = "eno1"/iface = "{}"/' Vagrantfile"#,
                iface
            )
            .cwd(vagrant_path),
        )?;
        Ok(())
    }
}

pub mod exp00002 {
    pub use super::exp00000::{
        connect_and_setup_host_and_vagrant, connect_and_setup_host_only, connect_to_vagrant,
        gen_vagrantfile, initial_reboot, start_vagrant, turn_off_swapdevs, turn_on_swapdevs,
        turn_on_zswap, virsh_vcpupin, VAGRANT_CORES, VAGRANT_MEM, VAGRANT_PORT,
        VAGRANT_RESULTS_DIR,
    };
    pub use super::{Login, Username};
}

pub mod exp00003 {
    pub use super::exp00000::*;
}

pub mod exp00004 {
    pub use super::exp00003::*;
}
