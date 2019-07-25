//! Run a workload on bare-metal (e.g. AWS).
//!
//! Requires `setup00000` with the appropriate kernel.

use clap::clap_app;

use serde::{Deserialize, Serialize};

use spurs::{
    cmd,
    ssh::{Execute, SshShell},
    util::escape_for_bash,
};

use crate::{
    common::{exp_0sim::*, get_cpu_freq, get_user_home_dir, output::OutputManager, paths::*},
    settings,
    workloads::{
        run_locality_mem_access, run_memcached_gen_data, run_time_loop, run_time_mmap_touch,
        LocalityMemAccessConfig, LocalityMemAccessMode, MemcachedWorkloadConfig,
        TimeMmapTouchConfig, TimeMmapTouchPattern,
    },
};

#[derive(Copy, Clone, Debug, Serialize, Deserialize)]
enum Workload {
    TimeLoop {
        n: usize,
    },
    LocalityMemAccess {
        n: usize,
    },
    TimeMmapTouch {
        size: usize,
        pattern: TimeMmapTouchPattern,
    },
    Memcached {
        size: usize,
    },
}

pub fn cli_options() -> clap::App<'static, 'static> {
    fn is_usize(s: String) -> Result<(), String> {
        s.as_str()
            .parse::<usize>()
            .map(|_| ())
            .map_err(|e| format!("{:?}", e))
    }

    clap_app! { exp00010 =>
        (about: "Run experiment 00010. Requires `sudo`.")
        (@arg HOSTNAME: +required +takes_value
         "The domain name of the remote (e.g. c240g2-031321.wisc.cloudlab.us:22)")
        (@arg USERNAME: +required +takes_value
         "The username on the remote (e.g. markm)")
        (@subcommand time_loop =>
            (about: "Run the `time_loop` workload.")
            (@arg N: +required +takes_value {is_usize}
             "The number of iterations of the workload (e.g. 50000000), preferably \
              divisible by 8 for `locality_mem_access`")
            )
        (@subcommand locality_mem_access =>
            (about: "Run the `locality_mem_access` workload.")
            (@arg N: +required +takes_value {is_usize}
             "The number of iterations of the workload (e.g. 50000000), preferably \
              divisible by 8 for `locality_mem_access`")
        )
        (@subcommand time_mmap_touch =>
            (about: "Run the `time_mmap_touch` workload.")
            (@arg SIZE: +required +takes_value {is_usize}
             "The number of GBs of the workload (e.g. 500)")
            (@group PATTERN =>
                (@attributes +required)
                (@arg zeros: -z "Fill pages with zeros")
                (@arg counter: -c "Fill pages with counter values")
            )
        )
        (@subcommand memcached =>
            (about: "Run the `memcached` workload.")
            (@arg SIZE: +required +takes_value {is_usize}
             "The number of GBs of the workload (e.g. 500)")
        )
    }
}

pub fn run(print_results_path: bool, sub_m: &clap::ArgMatches<'_>) -> Result<(), failure::Error> {
    let login = Login {
        username: Username(sub_m.value_of("USERNAME").unwrap()),
        hostname: sub_m.value_of("HOSTNAME").unwrap(),
        host: sub_m.value_of("HOSTNAME").unwrap(),
    };

    let (workload, workload_name, n, size, pattern) = match sub_m.subcommand() {
        ("time_loop", Some(sub_m)) => {
            let n = sub_m.value_of("N").unwrap().parse::<usize>().unwrap();
            (Workload::TimeLoop { n }, "time_loop", n, 0, None)
        }

        ("locality_mem_access", Some(sub_m)) => {
            let n = sub_m.value_of("N").unwrap().parse::<usize>().unwrap();
            (
                Workload::LocalityMemAccess { n },
                "locality_mem_access",
                n,
                0,
                None,
            )
        }

        ("time_mmap_touch", Some(sub_m)) => {
            let size = sub_m.value_of("SIZE").unwrap().parse::<usize>().unwrap();

            let pattern = if sub_m.is_present("zeros") {
                TimeMmapTouchPattern::Zeros
            } else {
                TimeMmapTouchPattern::Counter
            };

            (
                Workload::TimeMmapTouch { size, pattern },
                "time_mmap_touch",
                0,
                size,
                Some(pattern),
            )
        }

        ("memcached", Some(sub_m)) => {
            let size = sub_m.value_of("SIZE").unwrap().parse::<usize>().unwrap();

            (Workload::Memcached { size }, "memcached", 0, size, None)
        }

        _ => unreachable!(),
    };

    let ushell = SshShell::with_default_key(&login.username.as_str(), &login.host)?;
    let local_git_hash = crate::common::local_research_workspace_git_hash()?;
    let remote_git_hash = crate::common::research_workspace_git_hash(&ushell)?;
    let remote_research_settings = crate::common::get_remote_research_settings(&ushell)?;

    let settings = settings! {
        * workload: "bare_metal",
        * app: workload_name,
        exp: 10,

        (n > 0) n: n,
        (size > 0) size: size,
        (pattern.is_some()) pattern: match pattern {
            Some(TimeMmapTouchPattern::Zeros) => "zeros",
            Some(TimeMmapTouchPattern::Counter) => "counter",
            None => "n/a",
        },

        workload_settings: workload,

        transparent_hugepage_enabled: "always",
        transparent_hugepage_defrag: "always",
        transparent_hugepage_khugepaged_defrag: 1,
        transparent_hugepage_khugepaged_alloc_sleep_ms: 1000,
        transparent_hugepage_khugepaged_scan_sleep_ms: 1000,

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
    let workload = settings.get::<Workload>("workload_settings");

    // Reboot
    initial_reboot(&login)?;

    // Connect
    let ushell = connect_and_setup_host_only(&login)?;

    let user_home = &get_user_home_dir(&ushell)?;
    let zerosim_exp_path = &dir!(
        user_home.as_str(),
        RESEARCH_WORKSPACE_PATH,
        ZEROSIM_EXPERIMENTS_SUBMODULE
    );

    // Collect timers on VM
    let mut timers = vec![];

    let (output_file, params_file, time_file, _sim_file) = settings.gen_standard_names();
    let params = serde_json::to_string(&settings)?;

    ushell.run(cmd!(
        "echo '{}' > {}",
        escape_for_bash(&params),
        dir!(
            user_home.as_str(),
            setup00000::HOSTNAME_SHARED_RESULTS_DIR,
            params_file
        )
    ))?;

    let cores = crate::common::get_num_cores(&ushell)?;
    let mut tctx = crate::workloads::TasksetCtx::new(cores);

    // Run the workload.
    match workload {
        Workload::TimeLoop { n } => {
            time!(
                timers,
                "Workload",
                run_time_loop(
                    &ushell,
                    zerosim_exp_path,
                    n,
                    &dir!(setup00000::HOSTNAME_SHARED_RESULTS_DIR, output_file),
                    /* eager */ false,
                    &mut tctx,
                )?
            );
        }

        Workload::LocalityMemAccess { n } => {
            let local_file = settings.gen_file_name("local");
            let nonlocal_file = settings.gen_file_name("nonlocal");

            time!(timers, "Workload", {
                run_locality_mem_access(
                    &ushell,
                    &LocalityMemAccessConfig::default()
                        .exp_dir(zerosim_exp_path)
                        .locality(LocalityMemAccessMode::Local)
                        .n(n)
                        .threads(None)
                        .output_file(&dir!(setup00000::HOSTNAME_SHARED_RESULTS_DIR, local_file))
                        .eager(false),
                )?;
                run_locality_mem_access(
                    &ushell,
                    &LocalityMemAccessConfig::default()
                        .exp_dir(zerosim_exp_path)
                        .locality(LocalityMemAccessMode::Random)
                        .n(n)
                        .threads(None)
                        .output_file(&dir!(
                            setup00000::HOSTNAME_SHARED_RESULTS_DIR,
                            nonlocal_file
                        ))
                        .eager(false),
                )?;
            });
        }

        Workload::TimeMmapTouch { size, pattern } => {
            time!(
                timers,
                "Workload",
                run_time_mmap_touch(
                    &ushell,
                    &TimeMmapTouchConfig::default()
                        .exp_dir(zerosim_exp_path)
                        .pages((size << 30) >> 12)
                        .pattern(pattern)
                        .prefault(false)
                        .pf_time(None)
                        .output_file(Some(&dir!(
                            setup00000::HOSTNAME_SHARED_RESULTS_DIR,
                            output_file
                        )))
                        .eager(false)
                        .pin_core(tctx.next())
                )?
            );
        }

        Workload::Memcached { size } => {
            let freq = get_cpu_freq(&ushell)?;

            time!(
                timers,
                "Workload",
                run_memcached_gen_data(
                    &ushell,
                    &MemcachedWorkloadConfig::default()
                        .user("vagrant")
                        .exp_dir(zerosim_exp_path)
                        .server_size_mb(size << 10)
                        .wk_size_gb(size)
                        .freq(Some(freq))
                        .allow_oom(true)
                        .pf_time(None)
                        .output_file(Some(&dir!(
                            setup00000::HOSTNAME_SHARED_RESULTS_DIR,
                            output_file
                        )))
                        .eager(false)
                        .client_pin_core(tctx.next())
                        .server_pin_core(None)
                )?
            );
        }
    }

    ushell.run(cmd!("date"))?;

    ushell.run(cmd!("free -h"))?;

    ushell.run(cmd!(
        "echo -e '{}' > {}",
        crate::common::timings_str(timers.as_slice()),
        dir!(setup00000::HOSTNAME_SHARED_RESULTS_DIR, time_file)
    ))?;

    if print_results_path {
        let glob = settings.gen_file_name("*");
        println!("RESULTS: {}", glob);
    }

    Ok(())
}
