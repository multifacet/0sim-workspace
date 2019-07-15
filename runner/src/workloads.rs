//! Common workloads.

use bitflags::bitflags;

use serde::{Deserialize, Serialize};

use spurs::{
    cmd,
    ssh::{Execute, SshShell, SshSpawnHandle},
};

macro_rules! impl_conf {
    ($name:ident : $ty:ty) => {
        pub fn $name(self, $name: $ty) -> Self {
            Self { $name, ..self }
        }
    }
}

/// Set the apriori paging process using Swapnil's program. Requires `sudo`.
///
/// For example, to cause `ls` to be eagerly paged:
///
/// ```rust,ignore
/// setup_apriori_paging_process(&shell, "ls")?;
/// ```
pub fn setup_apriori_paging_process(shell: &SshShell, prog: &str) -> Result<(), failure::Error> {
    shell.run(cmd!(
        "{}/{}/apriori_paging_set_process {}",
        crate::common::paths::RESEARCH_WORKSPACE_PATH,
        crate::common::paths::ZEROSIM_SWAPNIL_PATH,
        prog
    ))?;
    Ok(())
}

/// Keeps track of which guest vCPUs have been assigned.
pub struct TasksetCtx {
    /// The total number of vCPUs.
    ncores: usize,

    /// The number of assignments so far.
    next: usize,
}

impl TasksetCtx {
    /// Create a new context with the given total number of cores.
    pub fn new(ncores: usize) -> Self {
        assert!(ncores > 0);
        TasksetCtx { ncores, next: 0 }
    }

    /// Get the next core (wrapping around to 0 if all cores have been assigned).
    pub fn next(&mut self) -> usize {
        let c = self.next % self.ncores;
        self.next += 1;
        c
    }
}

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
/// - `eager` indicates whether the workload should be run with eager paging.
pub fn run_time_mmap_touch(
    shell: &SshShell,
    exp_dir: &str,
    pages: usize,
    pattern: TimeMmapTouchPattern,
    prefault: bool,
    pf_time: Option<u64>,
    output_file: Option<&str>,
    eager: bool,
    tctx: &mut TasksetCtx,
) -> Result<(), failure::Error> {
    let pattern = match pattern {
        TimeMmapTouchPattern::Counter => "-c",
        TimeMmapTouchPattern::Zeros => "-z",
    };

    if eager {
        setup_apriori_paging_process(shell, "time_mmap_touch")?;
    }

    shell.run(
        cmd!(
            "sudo taskset -c {} ./target/release/time_mmap_touch {} {} {} {} > {}",
            tctx.next(),
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

/// The configuration of a memcached workload.
pub struct MemcachedWorkloadConfig<'s> {
    /// The path of the `0sim-experiments` submodule on the remote.
    exp_dir: &'s str,

    /// The user to run the `memcached` server as.
    user: &'s str,
    /// The size of `memcached` server in MB.
    server_size_mb: usize,
    /// Specifies whether the memcached server is allowed to OOM.
    allow_oom: bool,

    /// The core number that the memcached server is pinned to, if any.
    server_pin_core: Option<usize>,
    /// The core number that the workload client is pinned to.
    client_pin_core: usize,

    /// The size of the workload in GB.
    wk_size_gb: usize,
    /// The file to which the workload will write its output. If `None`, then `/dev/null` is used.
    output_file: Option<&'s str>,

    /// The CPU frequency. If passed, the workload will use rdtsc for timing.
    freq: Option<usize>,
    /// Specifies the page fault time if TSC offsetting is to try to account for it.
    pf_time: Option<u64>,
    /// Indicates whether the workload should be run with eager paging.
    eager: bool,
}

impl Default for MemcachedWorkloadConfig<'_> {
    fn default() -> Self {
        Self {
            exp_dir: "",
            user: "",
            server_size_mb: 0,
            allow_oom: false,
            wk_size_gb: 0,
            output_file: None,
            server_pin_core: None,
            client_pin_core: 0,
            freq: None,
            pf_time: None,
            eager: false,
        }
    }
}

impl<'s> MemcachedWorkloadConfig<'s> {
    impl_conf! {exp_dir: &'s str}
    impl_conf! {user: &'s str}
    impl_conf! {server_size_mb: usize}
    impl_conf! {allow_oom: bool}
    impl_conf! {wk_size_gb: usize}
    impl_conf! {output_file: Option<&'s str>}
    impl_conf! {server_pin_core: Option<usize>}
    impl_conf! {client_pin_core: usize}
    impl_conf! {freq: Option<usize>}
    impl_conf! {pf_time: Option<u64>}
    impl_conf! {eager: bool}
}

/// Start a `memcached` server in daemon mode as the given user with the given amount of memory.
/// Usually this is called indirectly through one of the other workload routines.
///
/// `allow_oom` specifies whether memcached is allowed to OOM. This gives much simpler performance
/// behaviors. memcached uses a large amount of the memory you give it for bookkeeping, rather
/// than user data, so OOM will almost certainly happen.
///
/// `eager` indicates whether the workload should be run with eager paging.
pub fn start_memcached(
    shell: &SshShell,
    cfg: &MemcachedWorkloadConfig<'_>,
) -> Result<(), failure::Error> {
    if cfg.eager {
        setup_apriori_paging_process(shell, "memcached")?;
    }

    if let Some(server_pin_core) = cfg.server_pin_core {
        shell.run(cmd!(
            "taskset -c {} memcached {} -m {} -d -u {}",
            server_pin_core,
            if cfg.allow_oom { "-M" } else { "" },
            cfg.server_size_mb,
            cfg.user
        ))?
    } else {
        shell.run(cmd!(
            "memcached {} -m {} -d -u {}",
            if cfg.allow_oom { "-M" } else { "" },
            cfg.server_size_mb,
            cfg.user
        ))?
    };
    Ok(())
}

/// Run the `memcached_gen_data` workload.
pub fn run_memcached_gen_data(
    shell: &SshShell,
    cfg: &MemcachedWorkloadConfig<'_>,
) -> Result<(), failure::Error> {
    // Start server
    start_memcached(&shell, cfg)?;

    // Run workload
    let cmd = cmd!(
        "taskset -c {} ./target/release/memcached_gen_data localhost:11211 {} {} {} | tee {}",
        cfg.client_pin_core,
        cfg.wk_size_gb,
        if let Some(freq) = cfg.freq {
            format!("--freq {}", freq)
        } else {
            "".into()
        },
        if let Some(pf_time) = cfg.pf_time {
            format!("--pftime {}", pf_time)
        } else {
            "".into()
        },
        cfg.output_file.unwrap_or("/dev/null")
    )
    .cwd(cfg.exp_dir);

    let cmd = if cfg.allow_oom {
        cmd.allow_error()
    } else {
        cmd
    };

    shell.run(cmd)?;

    Ok(())
}

/// Run the `memcached_gen_data` workload.
///
/// - `interval` is the interval at which to collect THP stats.
/// - `continual_compaction` specifies whether spurious failures are employed and what type.
/// - `output_file` is the file to which the workload will write its output; note that,
///   `cfg.output_file` is the file to which memcached request latency are written.
pub fn run_memcached_and_capture_thp(
    shell: &SshShell,
    cfg: &MemcachedWorkloadConfig<'_>,
    interval: usize,
    continual_compaction: Option<usize>,
    output_file: &str,
) -> Result<(), failure::Error> {
    // Start server
    start_memcached(&shell, cfg)?;

    // Turn on/off spurious failures
    if let Some(mode) = continual_compaction {
        shell.run(cmd!("echo {} | sudo tee /proc/compact_spurious_fail", mode))?;
    } else {
        shell.run(cmd!("echo 0 | sudo tee /proc/compact_spurious_fail"))?;
    }

    // Run workload
    let cmd = cmd!(
        "taskset -c {} ./target/release/memcached_and_capture_thp localhost:11211 {} {} {} {} | tee {}",
        cfg.client_pin_core,
        cfg.wk_size_gb,
        interval,
        cfg.output_file.unwrap_or("/dev/null"),
        if continual_compaction.is_some() {
            "--continual_compaction"
        } else {
            ""
        },
        output_file
    )
    .cwd(cfg.exp_dir)
    .use_bash();

    let cmd = if cfg.allow_oom {
        cmd.allow_error()
    } else {
        cmd
    };

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
/// - `eager` indicates whether the workload should be run with eager paging.
pub fn run_nas_cg(
    shell: &SshShell,
    zerosim_bmk_path: &str,
    class: NasClass,
    output_file: Option<&str>,
    eager: bool,
    tctx: &mut TasksetCtx,
) -> Result<(SshShell, SshSpawnHandle), failure::Error> {
    let class = match class {
        NasClass::E => "E",
    };

    if eager {
        setup_apriori_paging_process(shell, &format!("cg.{}.x", class))?;
    }

    let handle = shell.spawn(
        cmd!(
            "taskset -c {} ./bin/cg.{}.x > {}",
            tctx.next(),
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
///   a value of `1`. If `None`, then run indefinitely.
/// - `size_kb` is the number of kilobytes to mmap and touch.
/// - `eager` indicates whether the workload should be run with eager paging.
pub fn run_memhog(
    shell: &SshShell,
    r: Option<usize>,
    size_kb: usize,
    opts: MemhogOptions,
    eager: bool,
    tctx: &mut TasksetCtx,
) -> Result<(SshShell, SshSpawnHandle), failure::Error> {
    if eager {
        setup_apriori_paging_process(shell, "memhog")?;
    }

    shell.spawn(cmd!(
        "{} ; do \
         taskset -c {} memhog -r1 {}k {} {} > /dev/null ; \
         done; \
         echo memhog done ;",
        tctx.next(),
        if let Some(r) = r {
            format!("for i in `seq {}`", r)
        } else {
            format!("while [ true ]")
        },
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
    ))
}

/// Run the `time_loop` microbenchmark on the remote.
///
/// - `exp_dir` is the path of the 0sim-experiments submodule.
/// - `n` is the number of times to loop.
/// - `output_file` is the location to put the output.
/// - `eager` indicates whether the workload should be run with eager paging.
pub fn run_time_loop(
    shell: &SshShell,
    exp_dir: &str,
    n: usize,
    output_file: &str,
    eager: bool,
    tctx: &mut TasksetCtx,
) -> Result<(), failure::Error> {
    if eager {
        setup_apriori_paging_process(shell, "time_loop")?;
    }

    shell.run(
        cmd!(
            "sudo taskset -c {} ./target/release/time_loop {} > {}",
            tctx.next(),
            n,
            output_file
        )
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

/// Run the `locality_mem_access` workload on the remote of the given number of iterations.
///
/// If `threads` is `None`, a single-threaded workload is run. Otherwise, a multithreaded workload
/// is run.
pub fn run_locality_mem_access(
    shell: &SshShell,
    exp_dir: &str,
    locality: LocalityMemAccessMode,
    n: usize,
    threads: Option<usize>,
    output_file: &str,
    eager: bool,
    tctx: &mut TasksetCtx,
) -> Result<(), failure::Error> {
    let locality = match locality {
        LocalityMemAccessMode::Local => "-l",
        LocalityMemAccessMode::Random => "-n",
    };

    if eager {
        setup_apriori_paging_process(shell, "locality_mem_access")?;
    }

    shell.run(
        cmd!(
            "time sudo taskset -c {} ./target/release/locality_mem_access {} {} {} > {}",
            tctx.next(),
            locality,
            n,
            if let Some(threads) = threads {
                format!("-t {}", threads)
            } else {
                "".into()
            },
            output_file,
        )
        .cwd(exp_dir)
        .use_bash(),
    )?;

    Ok(())
}

pub struct RedisWorkloadHandles {
    pub server_shell: SshShell,
    pub server_spawn_handle: SshSpawnHandle,
    pub client_shell: SshShell,
    pub client_spawn_handle: SshSpawnHandle,
}

impl RedisWorkloadHandles {
    pub fn wait_for_client(self) -> Result<(), failure::Error> {
        self.client_spawn_handle.join()?;
        Ok(())
    }
}

/// Every setting of the redis workload.
pub struct RedisWorkloadConfig<'s> {
    /// The path of the `0sim-experiments` submodule on the remote.
    exp_dir: &'s str,

    /// The size of `redis` server in MB.
    server_size_mb: usize,
    /// The size of the workload in GB.
    wk_size_gb: usize,
    /// The file to which the workload will write its output. If `None`, then `/dev/null` is used.
    output_file: Option<&'s str>,

    /// The core number that the redis server is pinned to, if any.
    server_pin_core: Option<usize>,
    /// The core number that the workload client is pinned to.
    client_pin_core: usize,

    /// The CPU frequency. If passed, the workload will use rdtsc for timing.
    freq: Option<usize>,
    /// Specifies the page fault time if TSC offsetting is to try to account for it.
    pf_time: Option<u64>,
    /// Indicates whether the workload should be run with eager paging.
    eager: bool,
}

impl Default for RedisWorkloadConfig<'_> {
    fn default() -> Self {
        Self {
            exp_dir: "",
            server_size_mb: 0,
            wk_size_gb: 0,
            output_file: None,
            server_pin_core: None,
            client_pin_core: 0,
            freq: None,
            pf_time: None,
            eager: false,
        }
    }
}

impl<'s> RedisWorkloadConfig<'s> {
    impl_conf! {exp_dir: &'s str}
    impl_conf! {server_size_mb: usize}
    impl_conf! {wk_size_gb: usize}
    impl_conf! {output_file: Option<&'s str>}
    impl_conf! {server_pin_core: Option<usize>}
    impl_conf! {client_pin_core: usize}
    impl_conf! {freq: Option<usize>}
    impl_conf! {pf_time: Option<u64>}
    impl_conf! {eager: bool}
}

/// Spawn a `redis` server in a new shell with the given amount of memory and set some important
/// config settings. Usually this is called indirectly through one of the other workload routines.
///
/// In order for redis snapshots to work properly, we need to tell the kernel to overcommit memory.
/// This requires `sudo` access.
///
/// The redis server is listening at port 7777.
///
/// The caller should ensure that any previous RDB is deleted.
///
/// Returns the spawned shell.
pub fn start_redis(
    shell: &SshShell,
    cfg: &RedisWorkloadConfig<'_>,
) -> Result<(SshShell, SshSpawnHandle), failure::Error> {
    // Set overcommit
    shell.run(cmd!("echo 1 | sudo tee /proc/sys/vm/overcommit_memory"))?;

    if cfg.eager {
        setup_apriori_paging_process(shell, "redis-server")?;
    }

    // Start the redis server
    let handle = if let Some(server_pin_core) = cfg.server_pin_core {
        shell.spawn(cmd!(
            "taskset -c {} redis-server --port 7777 --loglevel warning",
            server_pin_core
        ))?
    } else {
        shell.spawn(cmd!("redis-server --port 7777 --loglevel warning"))?
    };

    // Wait for server to start
    loop {
        let res = shell.run(cmd!("redis-cli -p 7777 INFO"));
        if res.is_ok() {
            break;
        }
    }

    const REDIS_SNAPSHOT_FREQ_SECS: usize = 300;

    // Settings
    // - maxmemory amount + evict random keys when full
    // - save snapshots every 300 seconds if >= 1 key changed to the file /tmp/dump.rdb
    with_shell! { shell =>
        cmd!("redis-cli -p 7777 CONFIG SET maxmemory-policy allkeys-random"),
        cmd!("redis-cli -p 7777 CONFIG SET maxmemory {}mb", cfg.server_size_mb),

        cmd!("redis-cli -p 7777 CONFIG SET dir /tmp/"),
        cmd!("redis-cli -p 7777 CONFIG SET dbfilename dump.rdb"),
        cmd!("redis-cli -p 7777 CONFIG SET save \"{} 1\"", REDIS_SNAPSHOT_FREQ_SECS),
    }

    Ok(handle)
}

/// Run the `redis_gen_data` workload.
pub fn run_redis_gen_data(
    shell: &SshShell,
    cfg: &RedisWorkloadConfig<'_>,
) -> Result<RedisWorkloadHandles, failure::Error> {
    // Start server
    let (server_shell, server_spawn_handle) = start_redis(&shell, cfg)?;

    // Run workload
    let (client_shell, client_spawn_handle) = shell.spawn(
        cmd!(
            "taskset -c {} ./target/release/redis_gen_data localhost:7777 {} {} {} | tee {} ; echo redis_gen_data done",
            cfg.client_pin_core,
            cfg.wk_size_gb,
            if let Some(freq) = cfg.freq {
                format!("--freq {}", freq)
            } else {
                "".into()
            },
            if let Some(pf_time) = cfg.pf_time {
                format!("--pftime {}", pf_time)
            } else {
                "".into()
            },
            cfg.output_file.unwrap_or("/dev/null")
        )
        .cwd(cfg.exp_dir),
    )?;

    Ok(RedisWorkloadHandles {
        server_shell,
        server_spawn_handle,
        client_shell,
        client_spawn_handle,
    })
}

/// Run the metis matrix multiply workload with the given matrix dimensions (square matrix). This
/// workload takes a really long time, so we start it in a spawned shell and return the join handle
/// rather than waiting for the workload to return.
///
/// NOTE: The amount of virtual memory used by the workload is
///
///     `(dim * dim) * 4 * 2` bytes
///
/// so if you want a workload of size `t` GB, use `dim = sqrt(t << 27)`.
///
/// - `bmk_dir` is the path to the `Metis` directory in the workspace on the remote.
/// - `dim` is the dimension of the matrix (one side), which is assumed to be square.
/// - `eager` indicates whether the workload should be run with eager paging.
pub fn run_metis_matrix_mult(
    shell: &SshShell,
    bmk_dir: &str,
    dim: usize,
    eager: bool,
    tctx: &mut TasksetCtx,
) -> Result<(SshShell, SshSpawnHandle), failure::Error> {
    if eager {
        setup_apriori_paging_process(shell, "matrix_mult2")?;
    }

    shell.spawn(
        cmd!(
            "taskset -c {} ./obj/matrix_mult2 -q -o -l {} ; echo matrix_mult2 done ;",
            tctx.next(),
            dim
        )
        .cwd(bmk_dir),
    )
}

/// Run the mix workload which consists of splitting memory between
///
/// - 1 data-obliv memhog process with memory pinning (running indefinitely)
/// - 1 redis server and client pair. The redis server does snapshots every minute.
/// - 1 metis instance doing matrix multiplication
///
/// This workload runs until the redis subworkload completes.
///
/// Given a requested workload size of `size_gb` GB, each sub-workload gets 1/3 of the space.
///
/// - `exp_dir` is the path of the `0sim-experiments` submodule on the remote.
/// - `bmk_dir` is the path to the `Metis` directory in the workspace on the remote.
/// - `freq` is the _host_ CPU frequency in MHz.
/// - `size_gb` is the total amount of memory of the mix workload in GB.
/// - `eager` indicates whether the workload should be run with eager paging.
pub fn run_mix(
    shell: &SshShell,
    exp_dir: &str,
    bmk_dir: &str,
    freq: usize,
    size_gb: usize,
    eager: bool,
    tctx: &mut TasksetCtx,
) -> Result<(), failure::Error> {
    let redis_handles = run_redis_gen_data(
        shell,
        &RedisWorkloadConfig::default()
            .exp_dir(exp_dir)
            .server_size_mb((size_gb << 10) / 3)
            .wk_size_gb(size_gb / 3)
            .freq(Some(freq))
            .pf_time(None)
            .output_file(None)
            .eager(true)
            .client_pin_core(tctx.next())
            .server_pin_core(None),
    )?;

    let matrix_dim = (((size_gb / 3) << 27) as f64).sqrt() as usize;
    let _metis_handle = run_metis_matrix_mult(shell, bmk_dir, matrix_dim, eager, tctx)?;

    let _memhog_handles = run_memhog(
        shell,
        None,
        (size_gb << 20) / 3,
        MemhogOptions::PIN | MemhogOptions::DATA_OBLIV,
        eager,
        tctx,
    )?;

    // Wait for redis client to finish
    redis_handles.client_spawn_handle.join()?;

    Ok(())
}
