//! A library of routines commonly used in experiments.
//!
//! In honor of my friend Josh:
//!
//!  _━*━___━━___━__*___━_*___┓━╭━━━━━━━━━╮
//! __*_━━___━━___━━*____━━___┗┓|::::::^---^
//! ___━━___━*━___━━____━━*___━┗|::::|｡◕‿‿◕｡|
//! ___*━__━━_*___━━___*━━___*━━╰O­-O---O--O ╯

// Must be imported first because the other submodules use the macros defined therein.
#[macro_use]
mod macros;

#[macro_use]
pub mod output;

pub mod exp_0sim;

use serde::{Deserialize, Serialize};

use spurs::{
    cmd,
    ssh::{Execute, SshShell},
};

use paths::*;

#[derive(Copy, Clone, Debug)]
pub struct Username<'u>(pub &'u str);

impl Username<'_> {
    pub fn as_str(&self) -> &str {
        self.0
    }
}

#[derive(Clone, Debug)]
pub struct Login<'u, 'h, A: std::net::ToSocketAddrs + std::fmt::Display + Clone> {
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

/// Common paths.
pub mod paths {
    /// The path at which `clone_research_workspace` clones the workspace.
    pub const RESEARCH_WORKSPACE_PATH: &str = "research-workspace";

    /// Path to the 0sim submodule.
    pub const ZEROSIM_KERNEL_SUBMODULE: &str = "0sim";

    /// Path to the 0sim-experiments submodule.
    pub const ZEROSIM_EXPERIMENTS_SUBMODULE: &str = "0sim-experiments";

    /// Path to the 0sim-trace submodule.
    pub const ZEROSIM_TRACE_SUBMODULE: &str = "0sim-trace";

    /// Path to the HiBench submodule.
    pub const ZEROSIM_HIBENCH_SUBMODULE: &str = "bmks/zerosim-hadoop/HiBench";

    /// Path to the memhog (numactl) submodule.
    pub const ZEROSIM_MEMHOG_SUBMODULE: &str = "bmks/numactl";

    /// Path to the metis submodule.
    pub const ZEROSIM_METIS_SUBMODULE: &str = "bmks/Metis";

    /// Path to the memcached submodule.
    pub const ZEROSIM_MEMCACHED_SUBMODULE: &str = "bmks/memcached";

    /// Path the to nullfs submodule
    pub const ZEROSIM_NULLFS_SUBMODULE: &str = "bmks/nullfs";

    /// Path to redis.conf.
    pub const REDIS_CONF: &str = "bmks/redis.conf";

    /// Path to benchmarks directory.
    pub const ZEROSIM_BENCHMARKS_DIR: &str = "bmks";

    /// Path to Hadoop benchmark stuff within the benchmarks dir.
    pub const ZEROSIM_HADOOP_PATH: &str = "zerosim-hadoop";

    /// Path to Swapnil's scripts within the benchmarks dir.
    pub const ZEROSIM_SWAPNIL_PATH: &str = "swapnil_scripts";

    /// Path to the `vagrant` subdirectory where `gen_vagrantfile` will do its work.
    pub const VAGRANT_SUBDIRECTORY: &str = "vagrant";

    pub mod setup00000 {
        /// The shared directory on the host.
        pub const HOSTNAME_SHARED_DIR: &str = "vm_shared/";

        /// The shared directory for results on the host.
        pub const HOSTNAME_SHARED_RESULTS_DIR: &str = "vm_shared/results/";

        /// The shared directory on the guest.
        pub const VAGRANT_SHARED_DIR: &str = "/vagrant/vm_shared/";

        /// The shared directory for results on the guest.
        pub const VAGRANT_RESULTS_DIR: &str = "/vagrant/vm_shared/results/";

        /// The URL of the tarball used to build the guest kernel.
        pub const KERNEL_RECENT_TARBALL: &str =
            "https://cdn.kernel.org/pub/linux/kernel/v5.x/linux-5.1.4.tar.xz";

        /// The location of the tarball used to build the guest kernel.
        pub const KERNEL_RECENT_TARBALL_NAME: &str = "linux-5.1.4.tar.xz";
    }

    pub mod setup00001 {
        /// The guest swapfile.
        pub const VAGRANT_GUEST_SWAPFILE: &str = "/home/vagrant/swap";
    }
}

/// Given an array of timings, generate a human-readable string.
pub fn timings_str(timings: &[(&str, std::time::Duration)]) -> String {
    let mut s = String::new();
    for (label, d) in timings.iter() {
        s.push_str(&format!("{}: {:?}\n", label, d));
    }
    s
}

/// Clone the research-workspace and checkout the given submodules. The given token is used as the
/// Github personal access token.
///
/// If the repository is already cloned, it is updated (along with submodules).
///
/// Returns the git hash of the cloned repo.
///
/// *NOTE*: This function intentionally does not take the repo URL. It should always be the above.
pub fn clone_research_workspace(
    ushell: &SshShell,
    token: &str,
    submodules: &[&str],
) -> Result<String, failure::Error> {
    // Check if the repo is already cloned.
    if let Ok(_hash) = research_workspace_git_hash(&ushell) {
        // If so, just update it.
        with_shell! { ushell in &dir!(RESEARCH_WORKSPACE_PATH) =>
            cmd!("git pull"),
            cmd!("git submodule update"),
        }
    } else {
        // Clone the repo.
        let repo = GitHubRepo::Https {
            repo: RESEARCH_WORKSPACE_REPO.into(),
            token: Some((GITHUB_CLONE_USERNAME.into(), token.into())),
        };
        ushell.run(cmd!("git clone {}", repo))?;
    }

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

/// Generate a local version name from the git branch and hash.
///
/// If the branch name is longer than 15 characters, it is truncated. If the git hash is longer
/// than 15 characters, it is truncated.
pub fn gen_local_version(branch: &str, hash: &str) -> String {
    let branch_split = std::cmp::min(branch.len(), 15);
    let hash_split = std::cmp::min(hash.len(), 15);
    format!(
        "{}-{}",
        branch.split_at(branch_split).0.replace("_", "-"),
        hash.split_at(hash_split).0
    )
}

/// Generate a new vagrant domain name and update the Vagrantfile.
pub fn gen_new_vagrantdomain(shell: &SshShell) -> Result<(), failure::Error> {
    let vagrant_path = &format!("{}/{}", RESEARCH_WORKSPACE_PATH, VAGRANT_SUBDIRECTORY);
    let uniq = shell.run(cmd!("date | sha256sum | head -c 10"))?;
    let uniq = uniq.stdout.trim();
    shell.run(cmd!("sed -i 's/:test_vm/:test_vm_{}/' Vagrantfile", uniq).cwd(vagrant_path))?;
    Ok(())
}

/// Returns the number of processor cores on the machine.
pub fn get_num_cores(shell: &SshShell) -> Result<usize, failure::Error> {
    let nprocess = shell.run(cmd!("getconf _NPROCESSORS_ONLN"))?.stdout;
    let nprocess = nprocess.trim();

    let nprocess = nprocess.parse::<usize>()?;

    Ok(nprocess)
}

/// Get the max CPU frequency of the remote in MHz.
///
/// NOTE: this is not necessarily the current CPU freq. You need to set the scaling governor.
pub fn get_cpu_freq(shell: &SshShell) -> Result<usize, failure::Error> {
    let freq =
        shell.run(cmd!("lscpu | grep 'CPU max MHz' | grep -oE '[0-9]+' | head -n1").use_bash())?;
    let alt =
        shell.run(cmd!("lscpu | grep 'CPU MHz' | grep -oE '[0-9]+' | head -n1").use_bash())?;
    if freq.stdout.trim().is_empty() {
        Ok(alt.stdout.trim().parse::<usize>().unwrap())
    } else {
        Ok(freq.stdout.trim().parse::<usize>().unwrap())
    }
}

/// Turn on THP on the remote using the given settings. Requires `sudo`.
pub fn turn_on_thp(
    shell: &SshShell,
    transparent_hugepage_enabled: &str,
    transparent_hugepage_defrag: &str,
    transparent_hugepage_khugepaged_defrag: usize,
    transparent_hugepage_khugepaged_alloc_sleep_ms: usize,
    transparent_hugepage_khugepaged_scan_sleep_ms: usize,
) -> Result<(), failure::Error> {
    shell.run(
        cmd!(
            "echo {} | sudo tee /sys/kernel/mm/transparent_hugepage/enabled",
            transparent_hugepage_enabled
        )
        .use_bash(),
    )?;
    shell.run(
        cmd!(
            "echo {} | sudo tee /sys/kernel/mm/transparent_hugepage/defrag",
            transparent_hugepage_defrag
        )
        .use_bash(),
    )?;
    shell.run(
        cmd!(
            "echo {} | sudo tee /sys/kernel/mm/transparent_hugepage/khugepaged/defrag",
            transparent_hugepage_khugepaged_defrag
        )
        .use_bash(),
    )?;
    shell.run(
        cmd!("echo {} | sudo tee /sys/kernel/mm/transparent_hugepage/khugepaged/alloc_sleep_millisecs",
             transparent_hugepage_khugepaged_alloc_sleep_ms).use_bash(),
    )?;
    shell.run(
        cmd!("echo {} | sudo tee /sys/kernel/mm/transparent_hugepage/khugepaged/scan_sleep_millisecs",
             transparent_hugepage_khugepaged_scan_sleep_ms).use_bash(),
    )?;

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
    /// unpacked source directory is the same as the tarball name without the `.tar.gz`, `.tar.xz`,
    /// or `.tgz` extension.
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

pub fn get_absolute_path(shell: &SshShell, path: &str) -> Result<String, failure::Error> {
    Ok(shell.run(cmd!("pwd").cwd(path))?.stdout.trim().into())
}

/// Build a Linux kernel package (RPM or DEB). This command does not install the new kernel.
///
/// `kernel_local_version` is the kernel `LOCALVERSION` string to pass to `make` for the RPM, if
/// any.
pub fn build_kernel(
    ushell: &SshShell,
    source: KernelSrc,
    config: KernelConfig<'_>,
    kernel_local_version: Option<&str>,
    pkg_type: KernelPkgType,
) -> Result<(), failure::Error> {
    // Check out or unpack the source code, returning its absolute path.
    let source_path = match source {
        KernelSrc::Git {
            repo_path,
            git_branch,
        } => {
            ushell.run(cmd!("git checkout {}", git_branch).cwd(&repo_path))?;

            get_absolute_path(ushell, &repo_path)?
        }

        KernelSrc::Tar { tarball_path } => {
            ushell.run(cmd!("tar xvf {}", tarball_path))?;

            get_absolute_path(
                ushell,
                tarball_path
                    .trim_end_matches(".tar.gz")
                    .trim_end_matches(".tar.xz")
                    .trim_end_matches(".tgz"),
            )?
        }
    };

    // kbuild path.
    let kbuild_path = &format!("{}/kbuild", source_path);

    ushell.run(cmd!("mkdir -p {}", kbuild_path))?;

    // save old config if there is one.
    ushell.run(cmd!("cp .config config.bak").cwd(kbuild_path).allow_error())?;

    // configure the new kernel we are about to build.
    ushell.run(cmd!("make O={} defconfig", kbuild_path).cwd(&source_path))?;

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
                "sed -i '/{}=/s/{}=.*$/# {} is not set/' {}/.config",
                opt,
                opt,
                opt,
                kbuild_path
            ))?;
        }
    }

    // Compile with as many processors as we have.
    //
    // NOTE: for some reason, this sometimes fails the first time, so just do it again.
    //
    // NOTE: we pipe `yes` into make because in some cases the build will request updating some
    // aspects of the config in ways that `make oldconfig` does not address, such as to generate a
    // signing key.
    let nprocess = get_num_cores(ushell)?;

    let make_target = match pkg_type {
        KernelPkgType::Deb => "bindeb-pkg",
        KernelPkgType::Rpm => "binrpm-pkg",
    };

    ushell.run(
        cmd!(
            "yes '' | make -j {} {} {}",
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

/// Something that may be done to a service.
pub enum ServiceAction {
    Start,
    #[allow(dead_code)]
    Stop,
    Restart,
    Disable,
    #[allow(dead_code)]
    Enable,
}

/// Start, stop, enable, disable, or restart a service.
pub fn service(
    shell: &SshShell,
    service: &str,
    action: ServiceAction,
) -> Result<(), failure::Error> {
    match action {
        ServiceAction::Restart => {
            shell.run(cmd!("sudo service {} restart", service))?;
        }
        ServiceAction::Start => {
            shell.run(cmd!("sudo service {} start", service))?;
        }
        ServiceAction::Stop => {
            shell.run(cmd!("sudo service {} stop", service))?;
        }
        ServiceAction::Enable => {
            shell.run(cmd!("sudo systemctl enable {}", service))?;
        }
        ServiceAction::Disable => {
            shell.run(cmd!("sudo systemctl disable {}", service))?;
        }
    }

    Ok(())
}

pub fn service_is_running(shell: &SshShell, service: &str) -> Result<bool, failure::Error> {
    Ok(shell.run(cmd!("systemctl status {}", service)).is_ok())
}
