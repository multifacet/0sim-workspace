//! This file is for temporary experiments. If an experiment has long-term value, it should be
//! moved to another file and given an actual experiment number.
//!
//! Requires `setup00000`.

use clap::{clap_app, ArgMatches};

use spurs::{cmd, Execute, SshShell};
use spurs_util::escape_for_bash;

use crate::{
    common::{
        exp_0sim::*,
        output::OutputManager,
        paths::{setup00000::*, *},
    },
    settings,
    workloads::{
        run_locality_mem_access, run_memcached_gen_data, run_time_mmap_touch,
        LocalityMemAccessConfig, LocalityMemAccessMode, MemcachedWorkloadConfig,
        TimeMmapTouchConfig, TimeMmapTouchPattern,
    },
};

/// # of iterations for locality_mem_access workload
const LOCALITY_N: usize = 10_000;

#[derive(Copy, Clone, Debug)]
enum Workload {
    Memcached,
    Zeros,
    Counter,
    Locality,
    HiBenchWordcount,
}

impl Workload {
    pub fn to_str(&self) -> &str {
        match self {
            Workload::Memcached => "memcached_gen_data",
            Workload::Zeros | Workload::Counter => "time_mmap_touch",
            Workload::Locality => "locality_mem_access",
            Workload::HiBenchWordcount => "hibench_wordcount",
        }
    }

    pub fn from_str(s: &str, pat: Option<TimeMmapTouchPattern>) -> Self {
        match (s, pat) {
            ("memcached_gen_data", None) => Workload::Memcached,
            ("time_mmap_touch", Some(TimeMmapTouchPattern::Zeros)) => Workload::Zeros,
            ("time_mmap_touch", Some(TimeMmapTouchPattern::Counter)) => Workload::Counter,
            ("locality_mem_access", None) => Workload::Locality,
            ("hibench_wordcount", None) => Workload::HiBenchWordcount,
            _ => panic!("unknown workload: {:?} {:?}", s, pat),
        }
    }
}

pub fn cli_options() -> clap::App<'static, 'static> {
    fn is_usize(s: String) -> Result<(), String> {
        s.as_str()
            .parse::<usize>()
            .map(|_| ())
            .map_err(|e| format!("{:?}", e))
    }

    clap_app! { exptmp =>
        (about: "Run the temporary experiment.")
        (@arg HOSTNAME: +required +takes_value
         "The domain name of the remote (e.g. c240g2-031321.wisc.cloudlab.us:22)")
        (@arg USERNAME: +required +takes_value
         "The username on the remote (e.g. markm)")
        (@arg SIZE: +required +takes_value {is_usize}
         "The number of GBs of the workload (e.g. 500)")
        (@group PATTERN =>
            (@attributes +required)
            (@arg zeros: -z "Fill pages with zeros")
            (@arg counter: -c "Fill pages with counter values")
            (@arg memcached: -m "Run a memcached workload")
            (@arg locality: -l "Run the locality test workload")
            (@arg hibench_wordcount: -b "Run HiBench Wordcount")
        )
        (@arg VMSIZE: +takes_value {is_usize} -v --vm_size
         "The number of GBs of the VM (defaults to 1024) (e.g. 500)")
        (@arg CORES: +takes_value {is_usize} -C --cores
         "The number of cores of the VM (defaults to 1)")
        (@arg WARMUP: -w --warmup
         "Pass this flag to warmup the VM before running the main workload.")
        (@arg PFTIME: +takes_value {is_usize} --pftime
         "Pass this flag to set the pf_time value for the workload.")
    }
}

pub fn run(print_results_path: bool, sub_m: &ArgMatches<'_>) -> Result<(), failure::Error> {
    let login = Login {
        username: sub_m.value_of("USERNAME").unwrap(),
        hostname: sub_m.value_of("HOSTNAME").unwrap(),
        host: sub_m.value_of("HOSTNAME").unwrap(),
    };
    let size = sub_m.value_of("SIZE").unwrap().parse::<usize>().unwrap();
    let workload = if sub_m.is_present("memcached") {
        Workload::Memcached
    } else if sub_m.is_present("zeros") {
        Workload::Zeros
    } else if sub_m.is_present("counter") {
        Workload::Counter
    } else if sub_m.is_present("locality") {
        Workload::Locality
    } else if sub_m.is_present("hibench_wordcount") {
        Workload::HiBenchWordcount
    } else {
        panic!("unknown workload")
    };
    let vm_size = sub_m
        .value_of("VMSIZE")
        .map(|value| value.parse::<usize>().unwrap());
    let cores = sub_m
        .value_of("CORES")
        .map(|value| value.parse::<usize>().unwrap());
    let warmup = sub_m.is_present("WARMUP");

    let vm_size = if let Some(vm_size) = vm_size {
        vm_size
    } else {
        VAGRANT_MEM
    };

    let cores = if let Some(cores) = cores {
        cores
    } else {
        VAGRANT_CORES
    };

    let pf_time = sub_m
        .value_of("PFTIME")
        .map(|s| s.to_string().parse::<u64>().unwrap());

    let ushell = SshShell::with_default_key(login.username, login.host)?;
    let local_git_hash = crate::common::local_research_workspace_git_hash()?;
    let remote_git_hash = crate::common::research_workspace_git_hash(&ushell)?;
    let remote_research_settings = crate::common::get_remote_research_settings(&ushell)?;

    let settings = settings! {
        * workload: workload.to_str(),
        exp: "tmp",

        * size: size,
        pattern: match workload {
            Workload::Memcached | Workload::Locality | Workload::HiBenchWordcount => None,
            Workload::Zeros => Some(TimeMmapTouchPattern::Zeros),
            Workload::Counter => Some(TimeMmapTouchPattern::Counter),
        },
        calibrated: false,
        warmup: warmup,
        pf_time: pf_time,

        * vm_size: vm_size,
        cores: cores,

        zswap_max_pool_percent: 50,

        username: login.username,
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
    let size = settings.get::<usize>("size");
    let cores = settings.get::<usize>("cores");
    let pattern = settings.get::<Option<TimeMmapTouchPattern>>("pattern");
    let workload = Workload::from_str(settings.get::<&str>("workload"), pattern);
    let warmup = settings.get::<bool>("warmup");
    let calibrate = settings.get::<bool>("calibrated");
    let zswap_max_pool_percent = settings.get::<usize>("zswap_max_pool_percent");
    let pf_time = settings.get::<Option<u64>>("pf_time");

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
            ZEROSIM_SKIP_HALT,
            ZEROSIM_LAPIC_ADJUST
        )?
    );

    // Environment
    turn_on_zswap(&mut ushell)?;

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

    // let zerosim_path_host = &dir!(RESEARCH_WORKSPACE_PATH, ZEROSIM_KERNEL_SUBMODULE);

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
    //const WARM_UP_SIZE: usize = 50; // GB
    if warmup {
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
    let freq = crate::common::get_cpu_freq(&ushell)?;

    // Run the workload
    match workload {
        Workload::Zeros | Workload::Counter => {
            let pattern = pattern.unwrap();

            // const PERF_MEASURE_TIME: usize = 960; // seconds
            // let perf_output_early = settings.gen_file_name("perfdata0");
            // let spawn_handle0 = ushell.spawn(cmd!(
            //     "sudo taskset -c 3 {}/tools/perf/perf stat -C 0 -I 1000 \
            //      -e 'cycles,cache-misses,dTLB-load-misses,dTLB-store-misses,\
            //      page-faults,context-switches,vmscan:*,kvm:*' -o {} sleep {}",
            //     zerosim_path_host,
            //     dir!(HOSTNAME_SHARED_RESULTS_DIR,
            //     perf_output_early),
            //     PERF_MEASURE_TIME,
            // ))?;

            // Then, run the actual experiment
            time!(
                timers,
                "Workload",
                run_time_mmap_touch(
                    &vshell,
                    &TimeMmapTouchConfig {
                        exp_dir: zerosim_exp_path,
                        pages: (size << 30) >> 12,
                        pattern: pattern,
                        prefault: false,
                        pf_time: pf_time,
                        output_file: Some(&dir!(VAGRANT_RESULTS_DIR, output_file)),
                        eager: false,
                        pin_core: tctx.next(),
                    }
                )?
            );

            // let _ = spawn_handle0.join()?;
        }
        Workload::Memcached => {
            // // Measure host stats with perf while the workload is running. We measure at the beginning
            // // of the workload and later in the workload after the "cliff".
            // const PERF_MEASURE_TIME: usize = 50; // seconds
            // const PERF_LATE_DELAY_MS: usize = 85 * 1000; // ms

            // let perf_output_early = settings.gen_file_name("perfdata0");
            // let perf_output_late = settings.gen_file_name("perfdata1");

            // let spawn_handle0 = ushell.spawn(cmd!(
            //     "sudo taskset -c 2 {}/tools/perf/perf stat -C 0 -I 1000 \
            //      -e 'cycles,cache-misses,dTLB-load-misses,dTLB-store-misses,\
            //      page-faults,context-switches,vmscan:*,kvm:*' -o {} sleep {}",
            //     zerosim_path_host,
            //     dir!(HOSTNAME_SHARED_RESULTS_DIR,
            //     perf_output_early),
            //     PERF_MEASURE_TIME,
            // ))?;

            // let spawn_handle1 = ushell.spawn(cmd!(
            //     "sudo taskset -c 2 {}/tools/perf/perf stat -C 0 -I 1000 -D {} \
            //      -e 'cycles,cache-misses,dTLB-load-misses,dTLB-store-misses,\
            //      page-faults,context-switches,vmscan:*,kvm:*' -o {} sleep {}",
            //     zerosim_path_host,
            //     PERF_LATE_DELAY_MS,
            //     dir!(HOSTNAME_SHARED_RESULTS_DIR,
            //     perf_output_late),
            //     PERF_MEASURE_TIME,
            // ))?;

            time!(
                timers,
                "Start and Workload",
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
                        pf_time: pf_time,
                        output_file: Some(&dir!(VAGRANT_RESULTS_DIR, output_file)),
                        eager: false,
                        client_pin_core: tctx.next(),
                        server_pin_core: None,
                    }
                )?
            );

            // let _ = spawn_handle0.join()?;
            // let _ = spawn_handle1.join()?;
        }
        Workload::Locality => {
            // const PERF_MEASURE_TIME: usize = 960; // seconds

            // let perf_output_early = settings.gen_file_name("perfdata0");
            // let spawn_handle0 = ushell.spawn(cmd!(
            //     "sudo taskset -c 3 {}/tools/perf/perf stat -C 0 -I 1000 \
            //      -e 'cycles,cache-misses,dTLB-load-misses,dTLB-store-misses,\
            //      page-faults,context-switches,vmscan:*,kvm:*' -o {} sleep {}",
            //     zerosim_path_host,
            //     dir!(HOSTNAME_SHARED_RESULTS_DIR,
            //     perf_output_early),
            //     PERF_MEASURE_TIME,
            // ))?;

            let trace_output_local = settings.gen_file_name("tracelocal");
            let trace_output_nonlocal = settings.gen_file_name("tracenonlocal");
            let (_shell, spawn_handle0) = ushell.spawn(cmd!(
                "sudo taskset -c 3 {}/target/release/zerosim-trace trace {} {} {} -t {}",
                dir!(RESEARCH_WORKSPACE_PATH, ZEROSIM_TRACE_SUBMODULE),
                500,     // interval
                100_000, // buffer size
                dir!(HOSTNAME_SHARED_RESULTS_DIR, trace_output_local),
                pf_time.unwrap(),
            ))?;

            let output_local = settings.gen_file_name("local");
            let output_nonlocal = settings.gen_file_name("nonlocal");

            // Then, run the actual experiment.
            // 1) Do local accesses
            // 2) Do non-local accesses
            time!(
                timers,
                "Workload 1",
                run_locality_mem_access(
                    &vshell,
                    &LocalityMemAccessConfig {
                        exp_dir: zerosim_exp_path,
                        locality: LocalityMemAccessMode::Local,
                        n: LOCALITY_N,
                        threads: None,
                        output_file: &dir!(VAGRANT_RESULTS_DIR, output_local),
                        eager: false,
                    }
                )?
            );

            let _ = spawn_handle0.join()?;

            let (_shell, spawn_handle0) = ushell.spawn(cmd!(
                "sudo taskset -c 3 {}/target/release/zerosim-trace trace {} {} {} -t {}",
                dir!(RESEARCH_WORKSPACE_PATH, ZEROSIM_TRACE_SUBMODULE),
                500,     // interval
                100_000, // buffer size
                dir!(HOSTNAME_SHARED_RESULTS_DIR, trace_output_nonlocal),
                pf_time.unwrap(),
            ))?;

            time!(
                timers,
                "Workload 2",
                run_locality_mem_access(
                    &vshell,
                    &LocalityMemAccessConfig {
                        exp_dir: zerosim_exp_path,
                        locality: LocalityMemAccessMode::Random,
                        n: LOCALITY_N,
                        threads: None,
                        output_file: &dir!(VAGRANT_RESULTS_DIR, output_nonlocal),
                        eager: false,
                    }
                )?
            );

            let _ = spawn_handle0.join()?;
        }

        Workload::HiBenchWordcount => {
            let zerosim_hadoop = dir!(
                RESEARCH_WORKSPACE_PATH,
                ZEROSIM_BENCHMARKS_DIR,
                ZEROSIM_HADOOP_PATH
            );
            let hibench_home = dir!(&zerosim_hadoop, "HiBench");

            // Start hadoop
            vshell.run(cmd!("./start-all-standalone.sh").cwd(&zerosim_hadoop))?;

            // Prepare hadoop input
            vshell.run(
                cmd!("./bin/workloads/micro/wordcount/prepare/prepare.sh").cwd(&hibench_home),
            )?;

            // Run workload
            vshell.run(cmd!("./bin/workloads/micro/wordcount/hadoop/run.sh").cwd(&hibench_home))?;

            // Stop hadoop
            vshell.run(cmd!("./start-all-standalone.sh").cwd(&zerosim_hadoop))?;
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
