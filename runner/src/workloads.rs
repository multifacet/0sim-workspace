//! Common workloads.

use bitflags::bitflags;

use serde::{Deserialize, Serialize};

use spurs::{
    cmd,
    ssh::{Execute, SshShell, SshSpawnHandle},
};

/// Set the apriori paging process using Swapnil's program. Requires `sudo`.
///
/// This should be run only from a vagrant VM.
///
/// For example, to cause `ls` to be eagerly paged:
///
/// ```rust,ignore
/// vagrant_setup_apriori_paging_process(&shell, "ls")?;
/// ```
pub fn vagrant_setup_apriori_paging_process(
    shell: &SshShell,
    prog: &str,
) -> Result<(), failure::Error> {
    shell.run(cmd!(
        "{}/apriori_paging_set_process {}",
        dir![
            "/home/vagrant",
            crate::common::paths::RESEARCH_WORKSPACE_PATH,
            crate::common::paths::ZEROSIM_BENCHMARKS_DIR,
            crate::common::paths::ZEROSIM_SWAPNIL_PATH
        ],
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

/// Settings for a run of the `time_mmap_touch` workload.
pub struct TimeMmapTouchConfig<'s> {
    /// The path of the `0sim-experiments` submodule on the remote.
    pub exp_dir: &'s str,

    /// The number of _pages_ to touch.
    pub pages: usize,
    /// Specifies the pattern to write to the pages.
    pub pattern: TimeMmapTouchPattern,

    /// The file to which the workload will write its output. If `None`, then `/dev/null` is used.
    pub output_file: Option<&'s str>,

    /// The core to pin the workload to in the guest.
    pub pin_core: usize,
    /// Specifies whether to prefault memory or not (true = yes).
    pub prefault: bool,
    /// Specifies the page fault time if TSC offsetting is to try to account for it.
    pub pf_time: Option<u64>,
    /// Indicates whether the workload should be run with eager paging (only in VM).
    pub eager: bool,
}

/// Run the `time_mmap_touch` workload on the remote `shell`. Requires `sudo`.
pub fn run_time_mmap_touch(
    shell: &SshShell,
    cfg: &TimeMmapTouchConfig<'_>,
) -> Result<(), failure::Error> {
    let pattern = match cfg.pattern {
        TimeMmapTouchPattern::Counter => "-c",
        TimeMmapTouchPattern::Zeros => "-z",
    };

    if cfg.eager {
        vagrant_setup_apriori_paging_process(shell, "time_mmap_touch")?;
    }

    shell.run(
        cmd!(
            "sudo taskset -c {} ./target/release/time_mmap_touch {} {} {} {} > {}",
            cfg.pin_core,
            cfg.pages,
            pattern,
            if cfg.prefault { "-p" } else { "" },
            if let Some(pf_time) = cfg.pf_time {
                format!("--pftime {}", pf_time)
            } else {
                "".into()
            },
            cfg.output_file.unwrap_or("/dev/null")
        )
        .cwd(cfg.exp_dir)
        .use_bash(),
    )?;

    Ok(())
}

/// The configuration of a memcached workload.
pub struct MemcachedWorkloadConfig<'s> {
    /// The path of the `0sim-experiments` submodule on the remote.
    pub exp_dir: &'s str,
    /// The directory in which the memcached binary is contained.
    pub memcached: &'s str,

    /// The user to run the `memcached` server as.
    pub user: &'s str,
    /// The size of `memcached` server in MB.
    pub server_size_mb: usize,
    /// Specifies whether the memcached server is allowed to OOM.
    pub allow_oom: bool,

    /// The core number that the memcached server is pinned to, if any.
    pub server_pin_core: Option<usize>,
    /// The core number that the workload client is pinned to.
    pub client_pin_core: usize,

    /// The size of the workload in GB.
    pub wk_size_gb: usize,
    /// The file to which the workload will write its output. If `None`, then `/dev/null` is used.
    pub output_file: Option<&'s str>,

    /// The CPU frequency. If passed, the workload will use rdtsc for timing.
    pub freq: Option<usize>,
    /// Specifies the page fault time if TSC offsetting is to try to account for it.
    pub pf_time: Option<u64>,
    /// Indicates whether the workload should be run with eager paging.
    pub eager: bool,
}

/// Start a `memcached` server in daemon mode as the given user with the given amount of memory.
/// Usually this is called indirectly through one of the other workload routines.
///
/// `allow_oom` specifies whether memcached is allowed to OOM. This gives much simpler performance
/// behaviors. memcached uses a large amount of the memory you give it for bookkeeping, rather
/// than user data, so OOM will almost certainly happen.
///
/// `eager` indicates whether the workload should be run with eager paging (only in VM).
pub fn start_memcached(
    shell: &SshShell,
    cfg: &MemcachedWorkloadConfig<'_>,
) -> Result<(), failure::Error> {
    if cfg.eager {
        vagrant_setup_apriori_paging_process(shell, "memcached")?;
    }

    // We need to update the system vma limit because malloc may cause it to be hit for
    // large-memory systems.
    shell.run(cmd!("sudo sysctl -w vm.max_map_count={}", 1_000_000_000))?;

    if let Some(server_pin_core) = cfg.server_pin_core {
        shell.run(cmd!(
            "taskset -c {} {}/memcached {} -m {} -d -u {} -f 1.11",
            server_pin_core,
            cfg.memcached,
            if cfg.allow_oom { "-M" } else { "" },
            cfg.server_size_mb,
            cfg.user
        ))?
    } else {
        shell.run(cmd!(
            "{}/memcached {} -m {} -d -u {} -f 1.11",
            cfg.memcached,
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
        cfg.wk_size_gb - 1, // Avoid a OOM
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
/// - `eager` indicates whether the workload should be run with eager paging (only in VM).
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
        vagrant_setup_apriori_paging_process(shell, &format!("cg.{}.x", class))?;
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
/// - `exp_dir` is the path of the `numactl` benchmark directory.
/// - `r` is the number of times to call `memhog`, not the value of `-r`. `-r` is always passed a
///   value of `1`. If `None`, then run indefinitely.
/// - `size_kb` is the number of kilobytes to mmap and touch.
/// - `eager` indicates whether the workload should be run with eager paging (only in VM).
pub fn run_memhog(
    shell: &SshShell,
    exp_dir: &str,
    r: Option<usize>,
    size_kb: usize,
    opts: MemhogOptions,
    eager: bool,
    tctx: &mut TasksetCtx,
) -> Result<(SshShell, SshSpawnHandle), failure::Error> {
    if eager {
        vagrant_setup_apriori_paging_process(shell, "memhog")?;
    }

    shell.spawn(cmd!(
        "{} ; do \
         LD_LIBRARY_PATH={} taskset -c {} {}/memhog -r1 {}k {} {} > /dev/null ; \
         done; \
         echo memhog done ;",
        if let Some(r) = r {
            format!("for i in `seq {}`", r)
        } else {
            "while [ 1 ]".into()
        },
        exp_dir,
        tctx.next(),
        exp_dir,
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
/// - `eager` indicates whether the workload should be run with eager paging (only in VM).
pub fn run_time_loop(
    shell: &SshShell,
    exp_dir: &str,
    n: usize,
    output_file: &str,
    eager: bool,
    tctx: &mut TasksetCtx,
) -> Result<(), failure::Error> {
    if eager {
        vagrant_setup_apriori_paging_process(shell, "time_loop")?;
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

/// Settings for a single instance of the `locality_mem_access` workload.
pub struct LocalityMemAccessConfig<'s> {
    /// The path of the 0sim-experiments submodule.
    pub exp_dir: &'s str,

    /// Make local or non-local access patterns?
    pub locality: LocalityMemAccessMode,
    /// Number of accesses.
    pub n: usize,
    /// Turn on multithreading or not? And how many threads. Note that `None` is not the same as
    /// `Some(1)`, which has the main thread and 1 worker.
    pub threads: Option<usize>,

    /// The location to write the output for the workload.
    pub output_file: &'s str,

    /// Turn on eager paging.
    pub eager: bool,
}

/// Run the `locality_mem_access` workload on the remote of the given number of iterations.
///
/// If `threads` is `None`, a single-threaded workload is run. Otherwise, a multithreaded workload
/// is run. The workload does its own CPU affinity assignments.
///
/// `eager` should only be used in a VM.
pub fn run_locality_mem_access(
    shell: &SshShell,
    cfg: &LocalityMemAccessConfig<'_>,
) -> Result<(), failure::Error> {
    let locality = match cfg.locality {
        LocalityMemAccessMode::Local => "-l",
        LocalityMemAccessMode::Random => "-n",
    };

    if cfg.eager {
        vagrant_setup_apriori_paging_process(shell, "locality_mem_access")?;
    }

    shell.run(
        cmd!(
            "time sudo ./target/release/locality_mem_access {} {} {} > {}",
            locality,
            cfg.n,
            if let Some(threads) = cfg.threads {
                format!("-t {}", threads)
            } else {
                "".into()
            },
            cfg.output_file,
        )
        .cwd(cfg.exp_dir)
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
    pub exp_dir: &'s str,
    /// The path to the nullfs submodule on the remote.
    pub nullfs: &'s str,
    /// The path of the `redis.conf` file on the remote.
    pub redis_conf: &'s str,

    /// The size of `redis` server in MB.
    pub server_size_mb: usize,
    /// The size of the workload in GB.
    pub wk_size_gb: usize,
    /// The file to which the workload will write its output. If `None`, then `/dev/null` is used.
    pub output_file: Option<&'s str>,

    /// The core number that the redis server is pinned to, if any.
    pub server_pin_core: Option<usize>,
    /// The core number that the workload client is pinned to.
    pub client_pin_core: usize,

    /// The CPU frequency. If passed, the workload will use rdtsc for timing.
    pub freq: Option<usize>,
    /// Specifies the page fault time if TSC offsetting is to try to account for it.
    pub pf_time: Option<u64>,
    /// Indicates whether the workload should be run with eager paging.
    pub eager: bool,
}

/// Spawn a `redis` server in a new shell with the given amount of memory and set some important
/// config settings. Usually this is called indirectly through one of the other workload routines.
///
/// In order for redis snapshots to work properly, we need to tell the kernel to overcommit memory.
/// This requires `sudo` access.
///
/// We also
///     - delete any existing RDB files.
///     - set up a nullfs to use for the snapshot directory
///
/// `eager` should only be used in a VM.
///
/// Returns the spawned shell.
pub fn start_redis(
    shell: &SshShell,
    cfg: &RedisWorkloadConfig<'_>,
) -> Result<(SshShell, SshSpawnHandle), failure::Error> {
    // Set overcommit
    shell.run(cmd!("echo 1 | sudo tee /proc/sys/vm/overcommit_memory"))?;

    if cfg.eager {
        vagrant_setup_apriori_paging_process(shell, "redis-server")?;
    }

    // Delete any previous database
    shell.run(cmd!("rm -f /tmp/dump.rdb"))?;

    // Start nullfs
    shell.run(cmd!("sudo {}/nullfs /mnt/nullfs", cfg.nullfs))?;

    // Start the redis server
    let handle = if let Some(server_pin_core) = cfg.server_pin_core {
        shell.spawn(cmd!(
            "taskset -c {} redis-server {}",
            server_pin_core,
            cfg.redis_conf
        ))?
    } else {
        shell.spawn(cmd!("redis-server {}", cfg.redis_conf))?
    };

    // Wait for server to start
    loop {
        let res = shell.run(cmd!("redis-cli -s /tmp/redis.sock INFO"));
        if res.is_ok() {
            break;
        }
    }

    const REDIS_SNAPSHOT_FREQ_SECS: usize = 300;

    // Settings
    // - maxmemory amount + evict random keys when full
    // - save snapshots every 300 seconds if >= 1 key changed to the file /tmp/dump.rdb
    with_shell! { shell =>
        cmd!("redis-cli -s /tmp/redis.sock CONFIG SET maxmemory-policy allkeys-random"),
        cmd!("redis-cli -s /tmp/redis.sock CONFIG SET maxmemory {}mb", cfg.server_size_mb),

        cmd!("redis-cli -s /tmp/redis.sock CONFIG SET save \"{} 1\"", REDIS_SNAPSHOT_FREQ_SECS),
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
            "taskset -c {} ./target/release/redis_gen_data unix:/tmp/redis.sock \
             {} {} {} | tee {} ; echo redis_gen_data done",
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
/// - `eager` indicates whether the workload should be run with eager paging (only in VM).
pub fn run_metis_matrix_mult(
    shell: &SshShell,
    bmk_dir: &str,
    dim: usize,
    eager: bool,
    tctx: &mut TasksetCtx,
) -> Result<(SshShell, SshSpawnHandle), failure::Error> {
    if eager {
        vagrant_setup_apriori_paging_process(shell, "matrix_mult2")?;
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
/// - `metis_dir` is the path to the `Metis` directory in the workspace on the remote.
/// - `numactl_dir` is the path to the `numactl` directory in the workspace on the remote.
/// - `redis_conf` is the path to the `redis.conf` file on the remote.
/// - `freq` is the _host_ CPU frequency in MHz.
/// - `size_gb` is the total amount of memory of the mix workload in GB.
/// - `eager` indicates whether the workload should be run with eager paging.
pub fn run_mix(
    shell: &SshShell,
    exp_dir: &str,
    metis_dir: &str,
    numactl_dir: &str,
    nullfs_dir: &str,
    redis_conf: &str,
    freq: usize,
    size_gb: usize,
    eager: bool,
    tctx: &mut TasksetCtx,
) -> Result<(), failure::Error> {
    let redis_handles = run_redis_gen_data(
        shell,
        &RedisWorkloadConfig {
            exp_dir,
            nullfs: nullfs_dir,
            server_size_mb: (size_gb << 10) / 3,
            wk_size_gb: size_gb / 3,
            freq: Some(freq),
            pf_time: None,
            output_file: None,
            eager: true,
            client_pin_core: tctx.next(),
            server_pin_core: None,
            redis_conf,
        },
    )?;

    let matrix_dim = (((size_gb / 3) << 27) as f64).sqrt() as usize;
    let _metis_handle = run_metis_matrix_mult(shell, metis_dir, matrix_dim, eager, tctx)?;

    let _memhog_handles = run_memhog(
        shell,
        numactl_dir,
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
