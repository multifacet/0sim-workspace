//! This program runs different routines remotely. Which routine is chosen by passing different
//! command line arguments. certain routines require extra arguments.

// Useful common routines
mod common;

// Setup routines
mod setup00000;
mod setup00001;
mod setup00002;

mod manual;

// Experiment routines
mod exp00000;
mod exp00002;
mod exp00003;
mod exp00004;
mod exp00005;
mod exptmp;

use clap::clap_app;

use crate::common::{Login, Username};

fn run() -> Result<(), failure::Error> {
    let matches = clap_app! {runner =>
        (about: "This program runs different routines remotely. Which routine is chosen by passing
         different command line arguments. certain routines require extra arguments.")
        (@arg DRY: -d --dry_run "Don't actually execute commands. Just print what would run and exit.")
        (@subcommand setup00000 =>
            (about: "Sets up the given _centos_ cloudlab machine for use with vagrant. Requires `sudo`.")
            (@arg CLOUDLAB: +required +takes_value
             "The domain name of the remote (e.g. c240g2-031321.wisc.cloudlab.us:22)")
            (@arg USERNAME: +required +takes_value
             "The username on the remote (e.g. markm)")
            (@arg TOKEN: +required +takes_value
             "This is the Github personal token for cloning the repo.")
            (@arg DEVICE: +takes_value -d --device
             "(Optional) the device to format and use as a home directory (e.g. -d /dev/sda)")
            (@arg MAPPER_DEVICE: +takes_value -m --mapper_device
             "(Optional) the device to use with device mapper as a thinly-provisioned swap space (e.g. -d /dev/sda)")
            (@arg GIT_BRANCH: +takes_value -g --git_branch
             "(Optional) the git branch to compile the kernel from (e.g. -g markm_ztier)")
            (@arg ONLY_VM: -v --only_vm
             "(Optional) only setup the VM")
            (@arg SWAP_DEV: -s --swap +takes_value ...
             "(Optional) specify which devices to use as swap devices. By default all \
              unpartitioned, unmounted devices are used.")
        )
        (@subcommand setup00001 =>
            (about: "Sets up the given _centos_ VM for use exp00003. Requires `sudo`.")
            (@arg CLOUDLAB: +required +takes_value
             "The domain name of the remote (e.g. c240g2-031321.wisc.cloudlab.us:22)")
            (@arg USERNAME: +required +takes_value
             "The username on the remote (e.g. markm)")
            (@arg GIT_BRANCH: +required +takes_value
             "The git branch to compile the kernel from (e.g. markm_ztier)")
        )
        (@subcommand setup00002 =>
            (about: "Sets up the given _centos_ machine for use exp00004. Requires `sudo`.")
            (@arg CLOUDLAB: +required +takes_value
             "The domain name of the remote (e.g. c240g2-031321.wisc.cloudlab.us:22)")
            (@arg USERNAME: +required +takes_value
             "The username on the remote (e.g. markm)")
            (@arg TOKEN: +required +takes_value
             "This is the Github personal token for cloning the repo.")
            (@arg GIT_BRANCH: +takes_value -g --git_branch
             "(Optional) The git branch to compile the kernel from (e.g. markm_ztier)")
        )

        (@subcommand manual =>
            (about: "Perform some (non-strict) subset of the setup for an experiment. Requires `sudo`.")
            (@arg HOSTNAME: +required +takes_value
             "The domain name of the remote (e.g. c240g2-031321.wisc.cloudlab.us:22)")
            (@arg USERNAME: +required +takes_value
             "The username on the remote (e.g. markm)")
            (@arg REBOOT: --reboot
             "(Optional) If present, reboots the host machine.")
            (@arg SWAP: --setup_swap
             "(Optional) If present, setup swapping")
            (@arg PERF: --perfgov
             "(Optional) If present, set the scaling governor to \"performance\"")
            (@arg PRINTK: --printk +takes_value {is_usize}
             "(Optional) If present, set the printk logging level for dmesg. \
              0 = high-priority only. 7 = everything.")
            (@arg SSDSWAP: --ssdswap
             "(Optional) If present, turn on ssdswap.")
            (@arg VM: --vm
             "(Optional) Start the vagrant VM. Use other flags to set VM memory and vCPUS.")
            (@arg VMSIZE: --vm_size +takes_value {is_usize}
             "Only valid with --vm. The number of GBs of the VM (defaults to 1024) (e.g. 500)")
            (@arg VMCORES: --vm_cores +takes_value {is_usize}
             "Only valid with --vm. The number of cores of the VM (defaults to 1)")
            (@arg ZSWAP: --zswap +takes_value {is_usize}
             "(Optional) Turn on zswap with the given `max_pool_percent`")
        )

        (@subcommand exptmp =>
            (about: "Run the temporary experiment.")
            (@arg CLOUDLAB: +required +takes_value
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
            )
            (@arg VMSIZE: +takes_value {is_usize} -v --vm_size
             "The number of GBs of the VM (defaults to 1024) (e.g. 500)")
            (@arg CORES: +takes_value {is_usize} -C --cores
             "The number of cores of the VM (defaults to 1)")
            (@arg WARMUP: -w --warmup
             "Pass this flag to warmup the VM before running the main workload.")
            (@arg PFTIME: +takes_value {is_usize} --pftime
             "Pass this flag to set the pf_time value for the workload.")
        )
        (@subcommand exp00000 =>
            (about: "Run experiment 00000. Requires `sudo`.")
            (@arg CLOUDLAB: +required +takes_value
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
            )
            (@arg VMSIZE: +takes_value {is_usize} -v --vm_size
             "The number of GBs of the VM (defaults to 1024) (e.g. 500)")
            (@arg CORES: +takes_value {is_usize} -C --cores
             "The number of cores of the VM (defaults to 1)")
            (@arg WARMUP: -w --warmup
             "Pass this flag to warmup the VM before running the main workload.")
            (@arg PREFAULT: -p --prefault
             "Pass this flag to prefault memory before running the main workload \
             (ignored for memcached).")
        )
        (@subcommand exp00002 =>
            (about: "Run experiment 00002. Requires `sudo`.")
            (@arg CLOUDLAB: +required +takes_value
             "The domain name of the remote (e.g. c240g2-031321.wisc.cloudlab.us:22)")
            (@arg USERNAME: +required +takes_value
             "The username on the remote (e.g. markm)")
            (@arg N: +required +takes_value {is_usize}
             "The number of iterations of the workload (e.g. 50000000)")
            (@arg VMSIZE: +takes_value {is_usize} -v --vm_size
             "The number of GBs of the VM (defaults to 1024)")
            (@arg CORES: +takes_value {is_usize} -C --cores
             "The number of cores of the VM (defaults to 1)")
            (@arg WARMUP: -w --warmup
             "Pass this flag to warmup the VM before running the main workload.")
        )
        (@subcommand exp00003 =>
            (about: "Run experiment 00003. Requires `sudo`.")
            (@arg CLOUDLAB: +required +takes_value
             "The domain name of the remote (e.g. c240g2-031321.wisc.cloudlab.us:22)")
            (@arg USERNAME: +required +takes_value
             "The username on the remote (e.g. markm)")
            (@arg SIZE: +required +takes_value {is_usize}
             "The number of GBs of the workload (e.g. 500)")
            (@arg VMSIZE: +required +takes_value {is_usize}
             "The number of GBs of the VM (defaults to 1024) (e.g. 500)")
            (@arg CORES: +takes_value {is_usize} -C --cores
             "The number of cores of the VM (defaults to 1)")
        )
        (@subcommand exp00004 =>
            (about: "Run experiment 00004. Requires `sudo`.")
            (@arg CLOUDLAB: +required +takes_value
             "The domain name of the remote (e.g. c240g2-031321.wisc.cloudlab.us:22)")
            (@arg USERNAME: +required +takes_value
             "The username on the remote (e.g. markm)")
            (@arg SIZE: +required +takes_value {is_usize}
             "The number of GBs of the workload (e.g. 500)")
        )
        (@subcommand exp00005 =>
            (about: "Run experiment 00005. Requires `sudo`.")
            (@arg CLOUDLAB: +required +takes_value
             "The domain name of the remote (e.g. c240g2-031321.wisc.cloudlab.us:22)")
            (@arg USERNAME: +required +takes_value
             "The username on the remote (e.g. markm)")
            (@arg WARMUP: -w --warmup
             "Pass this flag to warmup the VM before running the main workload.")
            (@arg VMSIZE: +takes_value {is_usize}
             "The number of GBs of the VM (defaults to 2048)")
            (@arg CORES: +takes_value {is_usize} -C --cores
             "The number of cores of the VM (defaults to 1)")
        )
    }
    .setting(clap::AppSettings::SubcommandRequired)
    .setting(clap::AppSettings::DisableVersion)
    .get_matches();

    let dry_run = matches.is_present("DRY");

    match matches.subcommand() {
        ("setup00000", Some(sub_m)) => {
            let login = Login {
                username: Username(sub_m.value_of("USERNAME").unwrap()),
                hostname: sub_m.value_of("CLOUDLAB").unwrap(),
                host: sub_m.value_of("CLOUDLAB").unwrap(),
            };
            let device = sub_m.value_of("DEVICE");
            let mapper_device = sub_m.value_of("MAPPER_DEVICE");
            let git_branch = sub_m.value_of("GIT_BRANCH");
            let only_vm = sub_m.is_present("ONLY_VM");
            let token = sub_m.value_of("TOKEN").unwrap();
            let swap_devs = sub_m
                .values_of("SWAP_DEV")
                .map(|i| i.collect())
                .unwrap_or_else(|| vec![]);

            assert!(mapper_device.is_none() || swap_devs.is_empty());

            setup00000::run(
                dry_run,
                &login,
                device,
                mapper_device,
                git_branch,
                only_vm,
                token,
                swap_devs,
            )
        }
        ("setup00001", Some(sub_m)) => {
            let login = Login {
                username: Username(sub_m.value_of("USERNAME").unwrap()),
                hostname: sub_m.value_of("CLOUDLAB").unwrap(),
                host: sub_m.value_of("CLOUDLAB").unwrap(),
            };
            let git_branch = sub_m.value_of("GIT_BRANCH").unwrap();
            setup00001::run(dry_run, &login, git_branch)
        }
        ("setup00002", Some(sub_m)) => {
            let login = Login {
                username: Username(sub_m.value_of("USERNAME").unwrap()),
                hostname: sub_m.value_of("CLOUDLAB").unwrap(),
                host: sub_m.value_of("CLOUDLAB").unwrap(),
            };
            let git_branch = sub_m.value_of("GIT_BRANCH");
            let token = sub_m.value_of("TOKEN").unwrap();
            setup00002::run(dry_run, &login, git_branch, token)
        }

        ("manual", Some(sub_m)) => manual::run(dry_run, sub_m),

        ("exptmp", Some(sub_m)) => exptmp::run(dry_run, sub_m),
        ("exp00000", Some(sub_m)) => {
            let login = Login {
                username: Username(sub_m.value_of("USERNAME").unwrap()),
                hostname: sub_m.value_of("CLOUDLAB").unwrap(),
                host: sub_m.value_of("CLOUDLAB").unwrap(),
            };
            let gbs = sub_m.value_of("SIZE").unwrap().parse::<usize>().unwrap();
            let pattern = if sub_m.is_present("memcached") {
                None
            } else {
                Some(if sub_m.is_present("zeros") {
                    "-z"
                } else {
                    "-c"
                })
            };
            let vm_size = sub_m
                .value_of("VMSIZE")
                .map(|value| value.parse::<usize>().unwrap());
            let cores = sub_m
                .value_of("CORES")
                .map(|value| value.parse::<usize>().unwrap());
            let warmup = sub_m.is_present("WARMUP");
            let prefault = sub_m.is_present("PREFAULT");

            exp00000::run(
                dry_run, &login, gbs, pattern, vm_size, cores, warmup, prefault,
            )
        }
        ("exp00002", Some(sub_m)) => {
            let login = Login {
                username: Username(sub_m.value_of("USERNAME").unwrap()),
                hostname: sub_m.value_of("CLOUDLAB").unwrap(),
                host: sub_m.value_of("CLOUDLAB").unwrap(),
            };
            let n = sub_m.value_of("N").unwrap().parse::<usize>().unwrap();
            let vm_size = sub_m
                .value_of("VMSIZE")
                .map(|value| value.parse::<usize>().unwrap());
            let cores = sub_m
                .value_of("CORES")
                .map(|value| value.parse::<usize>().unwrap());
            let warmup = sub_m.is_present("WARMUP");

            exp00002::run(dry_run, &login, n, vm_size, cores, warmup)
        }
        ("exp00003", Some(sub_m)) => {
            let login = Login {
                username: Username(sub_m.value_of("USERNAME").unwrap()),
                hostname: sub_m.value_of("CLOUDLAB").unwrap(),
                host: sub_m.value_of("CLOUDLAB").unwrap(),
            };
            let gbs = sub_m.value_of("SIZE").unwrap().parse::<usize>().unwrap();
            let vm_size = sub_m.value_of("VMSIZE").unwrap().parse::<usize>().unwrap();
            let cores = sub_m
                .value_of("CORES")
                .map(|value| value.parse::<usize>().unwrap());

            exp00003::run(dry_run, &login, gbs, vm_size, cores)
        }
        ("exp00004", Some(sub_m)) => {
            let login = Login {
                username: Username(sub_m.value_of("USERNAME").unwrap()),
                hostname: sub_m.value_of("CLOUDLAB").unwrap(),
                host: sub_m.value_of("CLOUDLAB").unwrap(),
            };
            let gbs = sub_m.value_of("SIZE").unwrap().parse::<usize>().unwrap();

            exp00004::run(dry_run, &login, gbs)
        }
        ("exp00005", Some(sub_m)) => {
            let login = Login {
                username: Username(sub_m.value_of("USERNAME").unwrap()),
                hostname: sub_m.value_of("CLOUDLAB").unwrap(),
                host: sub_m.value_of("CLOUDLAB").unwrap(),
            };
            let vm_size = sub_m
                .value_of("VMSIZE")
                .map(|value| value.parse::<usize>().unwrap());
            let cores = sub_m
                .value_of("CORES")
                .map(|value| value.parse::<usize>().unwrap());
            let warmup = sub_m.is_present("WARMUP");

            exp00005::run(dry_run, &login, vm_size, cores, warmup)
        }

        _ => {
            unreachable!();
        }
    }
}

fn main() {
    env_logger::init();

    if let Err(e) = run() {
        println!("RUNNER ERROR {:?}", e);
    }
}

fn is_usize(s: String) -> Result<(), String> {
    s.as_str()
        .parse::<usize>()
        .map(|_| ())
        .map_err(|e| format!("{:?}", e))
}
