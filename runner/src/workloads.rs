//! Common workloads.

use bitflags::bitflags;

use spurs::{
    cmd,
    ssh::{Execute, SshShell, SshSpawnHandle},
};

/// The different patterns supported by the `time_mmap_touch` workload.
pub enum TimeMmapTouchPattern {
    Zeros,
    Counter,
}

/// Run the `time_mmap_touch` workload on the remote `shell`. Requires `sudo`.
///
/// - `exp_dir` is the path of the `0sim-experiments` submodule on the remote.
/// - `pages` is the number of _pages_ to touch.
/// - `pattern` specifies the pattern to write to the pages.
/// - `output_file` is the file to which the workload will write its output. If `None`, then
///   `/dev/null` is used.
pub fn run_time_mmap_touch(
    shell: &SshShell,
    exp_dir: &str,
    pages: usize,
    pattern: TimeMmapTouchPattern,
    output_file: Option<&str>,
) -> Result<(), failure::Error> {
    let pattern = match pattern {
        TimeMmapTouchPattern::Counter => "-c",
        TimeMmapTouchPattern::Zeros => "-z",
    };

    shell.run(
        cmd!(
            "sudo ./target/release/time_mmap_touch {} {} > {}",
            pages,
            pattern,
            output_file.unwrap_or("/dev/null")
        )
        .cwd(exp_dir)
        .use_bash(),
    )?;

    Ok(())
}

/// Start a `memcached` server in daemon mode as the given user with the given amount of memory.
/// Usually this is called indirectly through one of the other workload routines.
pub fn start_memcached(shell: &SshShell, size_mb: usize, user: &str) -> Result<(), failure::Error> {
    shell.run(cmd!("memcached -m {} -d -u {}", size_mb, user))?;
    Ok(())
}

/// Run the `memcached_gen_data` workload.
///
/// - `user` is the user to run the `memcached` server as.
/// - `exp_dir` is the path of the `0sim-experiments` submodule on the remote.
/// - `server_size_mb` is the size of `memcached` server in MB.
/// - `wk_size_gb` is the size of the workload in GB.
/// - `output_file` is the file to which the workload will write its output. If `None`, then
///   `/dev/null` is used.
pub fn run_memcached_gen_data(
    shell: &SshShell,
    user: &str,
    exp_dir: &str,
    server_size_mb: usize,
    wk_size_gb: usize,
    output_file: Option<&str>,
) -> Result<(), failure::Error> {
    // Start server
    start_memcached(&shell, server_size_mb, user)?;

    shell.run(
        cmd!(
            "./target/release/memcached_gen_data localhost:11211 {} > {}",
            wk_size_gb,
            output_file.unwrap_or("/dev/null")
        )
        .cwd(exp_dir),
    )?;

    Ok(())
}

/// NAS Parallel Benchmark workload size classes. See online documentation.
pub enum NasClass {
    E,
}

/// Start the NAS CG workload. It must already be compiled. This workload takes a really long time,
/// so we start it in a spawned shell and return the join handle rather than waiting for the
/// workload to return.
///
/// - `zerosim_bmk_path` is the path to the `bmks` directory of `research-workspace`.
/// - `output_file` is the file to which the workload will write its output. If `None`, then
///   `/dev/null` is used.
pub fn run_nas_cg(
    shell: &SshShell,
    zerosim_bmk_path: &str,
    class: NasClass,
    output_file: Option<&str>,
) -> Result<(SshShell, SshSpawnHandle), failure::Error> {
    let class = match class {
        NasClass::E => "E",
    };

    let handle = shell.spawn(
        cmd!(
            "./bin/cg.{}.x > {}",
            class,
            output_file.unwrap_or("/dev/null")
        )
        .cwd(&format!("{}/NPB3.4/NPB3.4-OMP", zerosim_bmk_path)),
    )?;

    Ok(handle)
}

bitflags! {
    pub struct MemhogOptions: u32 {
        /// Use pinned memory.
        const Pin = 1;

        /// Data-oblivious mode.
        const DataObliv = 1<<1;
    }
}

/// Run `memhog` on the remote.
///
/// - `r` is the number of times to call `memhog`, not the value of `-r`. `-r` is always passed
///   a value of `1`.
/// - `size_kb` is the number of kilobytes to mmap and touch.
pub fn run_memhog(
    shell: &SshShell,
    r: usize,
    size_kb: usize,
    opts: MemhogOptions,
) -> Result<(), failure::Error> {
    // Repeat workload multiple times
    for _ in 0..r {
        shell.run(cmd!(
            "memhog -r1 {}k {} {} > /dev/null",
            size_kb,
            if opts.contains(MemhogOptions::Pin) {
                "-p"
            } else {
                ""
            },
            if opts.contains(MemhogOptions::DataObliv) {
                unimplemented!()
            } else {
                ""
            },
        ))?;
    }

    Ok(())
}
