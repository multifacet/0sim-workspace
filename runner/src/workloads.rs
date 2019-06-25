//! Common workloads.

use bitflags::bitflags;

use serde::{Deserialize, Serialize};

use spurs::{
    cmd,
    ssh::{Execute, SshShell, SshSpawnHandle},
};

/// The different patterns supported by the `time_mmap_touch` workload.
#[derive(Debug, Copy, Clone, Serialize, Deserialize)]
pub enum TimeMmapTouchPattern {
    Zeros,
    Counter,
}

/// Run the `time_mmap_touch` workload on the remote `shell`. Requires `sudo`.
///
/// - `exp_dir` is the path of the `0sim-experiments` submodule on the remote.
/// - `pages` is the number of _pages_ to touch.
/// - `pattern` specifies the pattern to write to the pages.
/// - `prefault` specifies whether to prefault memory or not (true = yes).
/// - `pf_time` specifies the page fault time if TSC offsetting is to try to account for it.
/// - `output_file` is the file to which the workload will write its output. If `None`, then
///   `/dev/null` is used.
pub fn run_time_mmap_touch(
    shell: &SshShell,
    exp_dir: &str,
    pages: usize,
    pattern: TimeMmapTouchPattern,
    prefault: bool,
    pf_time: Option<u64>,
    output_file: Option<&str>,
) -> Result<(), failure::Error> {
    let pattern = match pattern {
        TimeMmapTouchPattern::Counter => "-c",
        TimeMmapTouchPattern::Zeros => "-z",
    };

    shell.run(
        cmd!(
            "sudo ./target/release/time_mmap_touch {} {} {} {} > {}",
            pages,
            pattern,
            if prefault { "-p" } else { "" },
            if let Some(pf_time) = pf_time {
                format!("--pftime {}", pf_time)
            } else {
                "".into()
            },
            output_file.unwrap_or("/dev/null")
        )
        .cwd(exp_dir)
        .use_bash(),
    )?;

    Ok(())
}

/// Start a `memcached` server in daemon mode as the given user with the given amount of memory.
/// Usually this is called indirectly through one of the other workload routines.
///
/// `allow_oom` specifies whether memcached is allowed to OOM. This gives much simpler performance
/// behaviors. memcached uses a large amount of the memory you give it for bookkeeping, rather
/// than user data, so OOM will almost certainly happen.
pub fn start_memcached(
    shell: &SshShell,
    size_mb: usize,
    user: &str,
    allow_oom: bool,
) -> Result<(), failure::Error> {
    shell.run(cmd!(
        "memcached {} -m {} -d -u {}",
        if allow_oom { "-M" } else { "" },
        size_mb,
        user
    ))?;
    Ok(())
}

/// Run the `memcached_gen_data` workload.
///
/// - `user` is the user to run the `memcached` server as.
/// - `exp_dir` is the path of the `0sim-experiments` submodule on the remote.
/// - `server_size_mb` is the size of `memcached` server in MB.
/// - `wk_size_gb` is the size of the workload in GB.
/// - `freq` is the CPU frequency. If passed, the workload will use rdtsc for timing.
/// - `allow_oom` specifies whether the memcached server is allowed to OOM.
/// - `pf_time` specifies the page fault time if TSC offsetting is to try to account for it.
/// - `output_file` is the file to which the workload will write its output. If `None`, then
///   `/dev/null` is used.
pub fn run_memcached_gen_data(
    shell: &SshShell,
    user: &str,
    exp_dir: &str,
    server_size_mb: usize,
    wk_size_gb: usize,
    freq: Option<usize>,
    allow_oom: bool,
    pf_time: Option<u64>,
    output_file: Option<&str>,
) -> Result<(), failure::Error> {
    // Start server
    start_memcached(&shell, server_size_mb, user, allow_oom)?;

    // Run workload
    let cmd = cmd!(
        "./target/release/memcached_gen_data localhost:11211 {} {} {} > {}",
        wk_size_gb,
        if let Some(freq) = freq {
            format!("--freq {}", freq)
        } else {
            "".into()
        },
        if let Some(pf_time) = pf_time {
            format!("--pftime {}", pf_time)
        } else {
            "".into()
        },
        output_file.unwrap_or("/dev/null")
    )
    .cwd(exp_dir);

    let cmd = if allow_oom { cmd.allow_error() } else { cmd };

    shell.run(cmd)?;

    Ok(())
}

/// Run the `memcached_gen_data` workload.
///
/// - `user` is the user to run the `memcached` server as.
/// - `exp_dir` is the path of the `0sim-experiments` submodule on the remote.
/// - `server_size_mb` is the size of `memcached` server in MB.
/// - `wk_size_gb` is the size of the workload in GB.
/// - `interval` is the interval at which to collect THP stats.
/// - `allow_oom` specifies whether the memcached server is allowed to OOM.
/// - `continual_compaction` specifies whether spurious failures are employed and what type.
/// - `timing_file` is the file to which memcached request latencies will be written. If `None`,
///    then `/dev/null` is used.
/// - `output_file` is the file to which the workload will write its output.
pub fn run_memcached_and_capture_thp(
    shell: &SshShell,
    user: &str,
    exp_dir: &str,
    server_size_mb: usize,
    wk_size_gb: usize,
    interval: usize,
    allow_oom: bool,
    continual_compaction: Option<usize>,
    timing_file: Option<&str>,
    output_file: &str,
) -> Result<(), failure::Error> {
    // Start server
    start_memcached(&shell, server_size_mb, user, allow_oom)?;

    // Turn on/off spurious failures
    if let Some(mode) = continual_compaction {
        shell.run(cmd!("echo {} | sudo tee /proc/compact_spurious_fail", mode))?;
    } else {
        shell.run(cmd!("echo 0 | sudo tee /proc/compact_spurious_fail"))?;
    }

    // Run workload
    let cmd = cmd!(
        "./target/release/memcached_and_capture_thp localhost:11211 {} {} {} {} > {}",
        wk_size_gb,
        interval,
        timing_file.unwrap_or("/dev/null"),
        if continual_compaction.is_some() {
            "--continual_compaction"
        } else {
            ""
        },
        output_file
    )
    .cwd(exp_dir)
    .use_bash();

    let cmd = if allow_oom { cmd.allow_error() } else { cmd };

    shell.run(cmd)?;

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
        const PIN = 1;

        /// Data-oblivious mode.
        const DATA_OBLIV = 1<<1;
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
            if opts.contains(MemhogOptions::PIN) {
                "-p"
            } else {
                ""
            },
            if opts.contains(MemhogOptions::DATA_OBLIV) {
                "-o"
            } else {
                ""
            },
        ))?;
    }

    Ok(())
}

/// Run the `time_loop` microbenchmark on the remote.
///
/// - `exp_dir` is the path of the 0sim-experiments submodule.
/// - `n` is the number of times to loop.
/// - `output_file` is the location to put the output.
pub fn run_time_loop(
    shell: &SshShell,
    exp_dir: &str,
    n: usize,
    output_file: &str,
) -> Result<(), failure::Error> {
    shell.run(
        cmd!("sudo ./target/release/time_loop {} > {}", n, output_file)
            .cwd(exp_dir)
            .use_bash(),
    )?;

    Ok(())
}

/// Different modes for the `locality_mem_access` workload.
pub enum LocalityMemAccessMode {
    /// Local accesses. Good cache and TLB behavior.
    Local,

    /// Non-local accesses. Poor cache and TLB behavior.
    Random,
}

/// Run the `locality_mem_access` workload on the remote.
pub fn run_locality_mem_access(
    shell: &SshShell,
    exp_dir: &str,
    locality: LocalityMemAccessMode,
    output_file: &str,
) -> Result<(), failure::Error> {
    let locality = match locality {
        LocalityMemAccessMode::Local => "-l",
        LocalityMemAccessMode::Random => "-n",
    };

    shell.run(
        cmd!(
            "time sudo ./target/release/locality_mem_access {} > {}",
            locality,
            output_file,
        )
        .cwd(exp_dir)
        .use_bash(),
    )?;

    Ok(())
}

/// Spawn a `redis` server in a new shell with the given amount of memory and set some important
/// config settings. Usually this is called indirectly through one of the other workload routines.
///
/// The redis server is listening at port 7777.
///
/// The caller should ensure that any previous RDB is deleted.
///
/// Returns the spawned shell.
pub fn start_redis(
    shell: &SshShell,
    size_mb: usize,
) -> Result<(SshShell, SshSpawnHandle), failure::Error> {
    // Start the redis server
    let handle = shell.spawn(cmd!("redis-server --port 7777 --loglevel warning"))?;

    // Wait for server to start
    loop {
        let res = shell.run(cmd!("redis-cli -p 7777 INFO"));
        if res.is_ok() {
            break;
        }
    }

    // Settings
    // - maxmemory amount + evict random keys when full
    // - save snapshots every 60 seconds if >= 1 key changed to the file /tmp/dump.rdb
    shell.run(cmd!(
        "redis-cli -p 7777 CONFIG SET maxmemory-policy allkeys-random"
    ))?;
    shell.run(cmd!("redis-cli -p 7777 CONFIG SET maxmemory {}mb", size_mb))?;

    shell.run(cmd!("redis-cli -p 7777 CONFIG SET dir /tmp/"))?;
    shell.run(cmd!("redis-cli -p 7777 CONFIG SET dbfilename dump.rdb"))?;
    shell.run(cmd!("redis-cli -p 7777 CONFIG SET save \"60 1\""))?;

    Ok(handle)
}

/// Run the `redis_gen_data` workload.
///
/// - `exp_dir` is the path of the `0sim-experiments` submodule on the remote.
/// - `server_size_mb` is the size of `redis` server in MB.
/// - `wk_size_gb` is the size of the workload in GB.
/// - `freq` is the CPU frequency. If passed, the workload will use rdtsc for timing.
/// - `pf_time` specifies the page fault time if TSC offsetting is to try to account for it.
/// - `output_file` is the file to which the workload will write its output. If `None`, then
///   `/dev/null` is used.
pub fn run_redis_gen_data(
    shell: &SshShell,
    exp_dir: &str,
    server_size_mb: usize,
    wk_size_gb: usize,
    freq: Option<usize>,
    pf_time: Option<u64>,
    output_file: Option<&str>,
) -> Result<(), failure::Error> {
    // Start server
    start_redis(&shell, server_size_mb)?;

    // Run workload
    shell.run(
        cmd!(
            "./target/release/redis_gen_data localhost:7777 {} {} {} > {}",
            wk_size_gb,
            if let Some(freq) = freq {
                format!("--freq {}", freq)
            } else {
                "".into()
            },
            if let Some(pf_time) = pf_time {
                format!("--pftime {}", pf_time)
            } else {
                "".into()
            },
            output_file.unwrap_or("/dev/null")
        )
        .cwd(exp_dir),
    )?;

    Ok(())
}
