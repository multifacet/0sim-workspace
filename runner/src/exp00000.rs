//! Run the given workload on the remote machine in simulation and record its results.
//!
//! Requires `setup00000`.

use clap::clap_app;

use serde::{Deserialize, Serialize};

use spurs::{
    cmd,
    ssh::{Execute, SshShell},
    util::escape_for_bash,
};

use crate::{
    common::{
        exp_0sim::*,
        get_cpu_freq,
        output::OutputManager,
        paths::{setup00000::*, *},
        Username,
    },
    settings,
    workloads::{
        run_memcached_gen_data, run_metis_matrix_mult, run_redis_gen_data, run_time_mmap_touch,
        MemcachedWorkloadConfig, RedisWorkloadConfig, TimeMmapTouchConfig, TimeMmapTouchPattern,
    },
};

#[derive(Copy, Clone, Debug, Serialize, Deserialize)]
enum Workload {
    Memcached,
    Redis,
    MatrixMult2,
    TimeMmapTouch,
}

pub fn cli_options() -> clap::App<'static, 'static> {
    fn is_usize(s: String) -> Result<(), String> {
        s.as_str()
            .parse::<usize>()
            .map(|_| ())
            .map_err(|e| format!("{:?}", e))
    }

    clap_app! { exp00000 =>
        (about: "Run experiment 00000. Requires `sudo`.")
        (@arg HOSTNAME: +required +takes_value
         "The domain name of the remote (e.g. c240g2-031321.wisc.cloudlab.us:22)")
        (@arg USERNAME: +required +takes_value
         "The username on the remote (e.g. markm)")
        (@arg VMSIZE: +required +takes_value {is_usize}
         "The number of GBs of the VM (e.g. 500)")
        (@arg CORES: +required +takes_value {is_usize}
         "The number of cores of the VM")
        (@group PATTERN =>
            (@attributes +required)
            (@arg zeros: -z "Run the time_mmap_touch workload with zeros")
            (@arg counter: -c "Run the time_mmap_touch workload with counter values")
            (@arg memcached: -m "Run a memcached workload")
            (@arg redis: -r "Run a redis workload")
            (@arg matrixmult: -M "Run the Metis matrix_mult2 workload")
        )
        (@arg WARMUP: -w --warmup
         "Pass this flag to warmup the VM before running the main workload.")
        (@arg PREFAULT: -p --prefault
         "Pass this flag to prefault memory before running the main workload \
         (ignored for memcached).")
        (@arg SIZE: -s --size +takes_value {is_usize}
         "The number of GBs of the workload (e.g. 500)")
        (@arg D: --d +takes_value {is_usize}
         "(Optional) Set /proc/zerosim_d")
        (@arg DELTA: --delta +takes_value {is_usize}
         "(Optional) Set /proc/zerosim_delta")
        (@arg SKIP_HALT: --skip_halt
         "(Optional) Set /proc/zerosim_skip_halt")
        (@arg LAPIC_ADJUST: --lapic_adjust
         "(Optional) Set /proc/zerosim_lapic_adjust")
    }
}

pub fn run(print_results_path: bool, sub_m: &clap::ArgMatches<'_>) -> Result<(), failure::Error> {
    let login = Login {
        username: Username(sub_m.value_of("USERNAME").unwrap()),
        hostname: sub_m.value_of("HOSTNAME").unwrap(),
        host: sub_m.value_of("HOSTNAME").unwrap(),
    };

    let vm_size = sub_m.value_of("VMSIZE").unwrap().parse::<usize>().unwrap();
    let cores = sub_m.value_of("CORES").unwrap().parse::<usize>().unwrap();

    let workload = if sub_m.is_present("memcached") {
        Workload::Memcached
    } else if sub_m.is_present("redis") {
        Workload::Redis
    } else if sub_m.is_present("matrixmult") {
        Workload::MatrixMult2
    } else if sub_m.is_present("zeros") {
        Workload::TimeMmapTouch
    } else if sub_m.is_present("counter") {
        Workload::TimeMmapTouch
    } else {
        unreachable!();
    };

    let pattern = if sub_m.is_present("zeros") || sub_m.is_present("counter") {
        Some(if sub_m.is_present("zeros") {
            TimeMmapTouchPattern::Zeros
        } else {
            TimeMmapTouchPattern::Counter
        })
    } else {
        None
    };

    let size = sub_m
        .value_of("SIZE")
        .map(|value| value.parse::<usize>().unwrap());
    let warmup = sub_m.is_present("WARMUP");
    let prefault = sub_m.is_present("PREFAULT");

    let zerosim_d = sub_m
        .value_of("D")
        .map(|value| value.parse::<usize>().unwrap())
        .unwrap_or(10_000_000);
    let zerosim_delta = sub_m
        .value_of("DELTA")
        .map(|value| value.parse::<usize>().unwrap())
        .unwrap_or(0);
    let zerosim_skip_halt = sub_m.is_present("SKIP_HALT");
    let zerosim_lapic_adjust = sub_m.is_present("LAPIC_ADJUST");

    let ushell = SshShell::with_default_key(&login.username.as_str(), &login.host)?;
    let local_git_hash = crate::common::local_research_workspace_git_hash()?;
    let remote_git_hash = crate::common::research_workspace_git_hash(&ushell)?;
    let remote_research_settings = crate::common::get_remote_research_settings(&ushell)?;

    let settings = settings! {
        * workload: "bmk",
        * app: workload,
        exp: 0,

        * vm_size: vm_size,
        (cores > 1) cores: cores,
        pattern: pattern,
        prefault: prefault,

        (size.is_some()) size: size,
        calibrated: false,
        warmup: warmup,

        zswap_max_pool_percent: 50,
        zerosim_d: zerosim_d,
        zerosim_delta: zerosim_delta,
        zerosim_skip_halt: zerosim_skip_halt,
        zerosim_lapic_adjust: zerosim_lapic_adjust,

        username: login.username.as_str(),
        host: login.hostname,

        local_git_hash: local_git_hash,
        remote_git_hash: remote_git_hash,

        remote_research_settings: remote_research_settings,
    };

    run_inner(print_results_path, &login, settings)
}

/// Run the experiment using the settings passed. Note that because the only thing we are passed
/// are the settings, we know that there is no information that is not recorded in the settings
/// file.
fn run_inner<A>(
    print_results_path: bool,
    login: &Login<A>,
    settings: OutputManager,
) -> Result<(), failure::Error>
where
    A: std::net::ToSocketAddrs + std::fmt::Display + std::fmt::Debug + Clone,
{
    let vm_size = settings.get::<usize>("vm_size");
    let cores = settings.get::<usize>("cores");
    let workload = settings.get::<Workload>("app");
    let pattern = settings.get::<Option<TimeMmapTouchPattern>>("pattern");
    let size = settings.get::<Option<usize>>("size");
    let warmup = settings.get::<bool>("warmup");
    let prefault = settings.get::<bool>("prefault");
    let calibrate = settings.get::<bool>("calibrated");
    let zswap_max_pool_percent = settings.get::<usize>("zswap_max_pool_percent");
    let zerosim_d = settings.get::<usize>("zerosim_d");
    let zerosim_delta = settings.get::<usize>("zerosim_delta");
    let zerosim_skip_halt = settings.get::<bool>("zerosim_skip_halt");
    let zerosim_lapic_adjust = settings.get::<bool>("zerosim_lapic_adjust");

    // Reboot
    initial_reboot(&login)?;

    // Connect to host
    let mut ushell = connect_and_setup_host_only(&login)?;

    // Turn on SSDSWAP.
    turn_on_ssdswap(&ushell)?;

    // Collect timers on VM
    let mut timers = vec![];

    // Start and connect to VM
    let vshell = time!(
        timers,
        "Start VM",
        start_vagrant(
            &ushell,
            &login.host,
            vm_size,
            cores,
            /* fast */ true,
            zerosim_skip_halt,
            zerosim_lapic_adjust,
        )?
    );

    // Environment
    turn_on_zswap(&mut ushell)?;
    set_zerosim_d(&ushell, zerosim_d)?;
    set_zerosim_delta(&ushell, zerosim_delta)?;

    ushell.run(
        cmd!(
            "echo {} | sudo tee /sys/module/zswap/parameters/max_pool_percent",
            zswap_max_pool_percent
        )
        .use_bash(),
    )?;

    let zerosim_exp_path = &dir!(
        "/home/vagrant",
        RESEARCH_WORKSPACE_PATH,
        ZEROSIM_EXPERIMENTS_SUBMODULE
    );

    let size = if let Some(size) = size {
        size // GB
    } else {
        // Get the amount of memory the guest thinks it has (in KB).
        let size = vshell
            .run(cmd!("grep MemAvailable /proc/meminfo | awk '{{print $2}}'").use_bash())?
            .stdout;
        size.trim().parse::<usize>().unwrap() >> 20 // turn into GB
    };

    // Calibrate
    if calibrate {
        time!(
            timers,
            "Calibrate",
            vshell.run(cmd!("sudo ./target/release/time_calibrate").cwd(zerosim_exp_path))?
        );
    }

    let (output_file, params_file, time_file, sim_file) = settings.gen_standard_names();
    let params = serde_json::to_string(&settings)?;

    vshell.run(cmd!(
        "echo '{}' > {}",
        escape_for_bash(&params),
        dir!(VAGRANT_RESULTS_DIR, params_file)
    ))?;

    let mut tctx = crate::workloads::TasksetCtx::new(cores);

    // Warm up
    if warmup {
        //const WARM_UP_SIZE: usize = 50; // GB
        const WARM_UP_PATTERN: TimeMmapTouchPattern = TimeMmapTouchPattern::Zeros;
        time!(
            timers,
            "Warmup",
            run_time_mmap_touch(
                &vshell,
                &TimeMmapTouchConfig {
                    exp_dir: zerosim_exp_path,
                    pages: (size << 30) >> 12,
                    pattern: WARM_UP_PATTERN,
                    prefault: false,
                    pf_time: None,
                    output_file: None,
                    eager: false,
                    pin_core: tctx.next(),
                }
            )?
        );
    }

    // We want to use rdtsc as the time source, so find the cpu freq:
    let freq = get_cpu_freq(&ushell)?;

    // Run memcached or time_touch_mmap
    match workload {
        Workload::TimeMmapTouch => {
            time!(
                timers,
                "Workload",
                run_time_mmap_touch(
                    &vshell,
                    &TimeMmapTouchConfig {
                        exp_dir: zerosim_exp_path,
                        pages: (size << 30) >> 12,
                        pattern: pattern.unwrap(),
                        prefault: prefault,
                        pf_time: None,
                        output_file: Some(&dir!(VAGRANT_RESULTS_DIR, output_file)),
                        eager: false,
                        pin_core: tctx.next(),
                    }
                )?
            );
        }

        Workload::Memcached => {
            time!(
                timers,
                "Workload",
                run_memcached_gen_data(
                    &vshell,
                    &MemcachedWorkloadConfig {
                        user: "vagrant",
                        exp_dir: zerosim_exp_path,
                        memcached: &dir!(
                            "/home/vagrant",
                            RESEARCH_WORKSPACE_PATH,
                            ZEROSIM_MEMCACHED_SUBMODULE
                        ),
                        server_size_mb: size << 10,
                        wk_size_gb: size,
                        freq: Some(freq),
                        allow_oom: true,
                        pf_time: None,
                        output_file: Some(&dir!(VAGRANT_RESULTS_DIR, output_file)),
                        eager: false,
                        client_pin_core: tctx.next(),
                        server_pin_core: None,
                    }
                )?
            );
        }

        Workload::Redis => {
            time!(
                timers,
                "Start and Workload",
                run_redis_gen_data(
                    &vshell,
                    &RedisWorkloadConfig {
                        exp_dir: zerosim_exp_path,
                        server_size_mb: size << 10,
                        wk_size_gb: size,
                        freq: Some(freq),
                        pf_time: None,
                        output_file: Some(&dir!(VAGRANT_RESULTS_DIR, output_file)),
                        eager: false,
                        client_pin_core: tctx.next(),
                        server_pin_core: None,
                        redis_conf: &dir!("/home/vagrant", RESEARCH_WORKSPACE_PATH, REDIS_CONF),
                        nullfs: &dir!(
                            "/home/vagrant",
                            RESEARCH_WORKSPACE_PATH,
                            ZEROSIM_NULLFS_SUBMODULE
                        )
                    }
                )?
                .wait_for_client()?
            );
        }

        Workload::MatrixMult2 => {
            time!(
                timers,
                "Workload",
                run_metis_matrix_mult(
                    &vshell,
                    &dir!(
                        "/home/vagrant",
                        RESEARCH_WORKSPACE_PATH,
                        ZEROSIM_METIS_SUBMODULE
                    ),
                    ((size << 27) as f64).sqrt() as usize,
                    /* eager */ false,
                    &mut tctx,
                )?
                .1
                .join()?
            );
        }
    }

    ushell.run(cmd!("date"))?;

    vshell.run(cmd!(
        "echo -e '{}' > {}",
        crate::common::timings_str(timers.as_slice()),
        dir!(VAGRANT_RESULTS_DIR, time_file)
    ))?;

    crate::common::exp_0sim::gen_standard_sim_output(&sim_file, &ushell, &vshell)?;

    if print_results_path {
        let glob = settings.gen_file_name("*");
        println!("RESULTS: {}", glob);
    }

    Ok(())
}
