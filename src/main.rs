//! This program runs different routines remotely. Which routine is chosen by passing different
//! command line arguments. certain routines require extra arguments.

// Setup routines
mod setup00000;

// Experiment routines
mod exp00000;

use clap::clap_app;

fn main() -> Result<(), failure::Error> {
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
            (@arg DEVICE: +takes_value -d --device
             "(Optional) the device to format and use as a home directory (e.g. -d /dev/sda)")
            (@arg GIT_BRANCH: +takes_value -g --git_branch
             "(Optional) the git branch to compile the kernel from (e.g. -g markm_ztier)")
        )
        (@subcommand exp00000 =>
            (about: "Run experiment 00000. Requires `sudo`.")
            (@arg CLOUDLAB: +required +takes_value
             "The domain name of the remote (e.g. c240g2-031321.wisc.cloudlab.us:22)")
            (@arg USERNAME: +required +takes_value
             "The username on the remote (e.g. markm)")
            (@arg SIZE: +required +takes_value {is_usize}
             "The number of GBs of the workload (e.g. markm)")
            (@group PATTERN =>
                (@attributes +required)
                (@arg zeros: -z "Fill pages with zeros")
                (@arg counter: -c "Fill pages with counter values")
            )
        )
    }
    .setting(clap::AppSettings::SubcommandRequired)
    .setting(clap::AppSettings::DisableVersion)
    .get_matches();

    let dry_run = matches.is_present("DRY");

    match matches.subcommand() {
        ("setup00000", Some(sub_m)) => {
            let cloudlab = sub_m.value_of("CLOUDLAB").unwrap();
            let username = sub_m.value_of("USERNAME").unwrap();
            let device = sub_m.value_of("DEVICE");
            let git_branch = sub_m.value_of("GIT_BRANCH");
            setup00000::run(dry_run, cloudlab, username, device, git_branch)
        }
        ("exp00000", Some(sub_m)) => {
            let cloudlab = sub_m.value_of("CLOUDLAB").unwrap();
            let username = sub_m.value_of("USERNAME").unwrap();
            let gbs = sub_m.value_of("SIZE").unwrap().parse::<usize>().unwrap();
            let pattern = if sub_m.is_present("zeros") {
                "-z"
            } else {
                "-c"
            };

            exp00000::run(dry_run, cloudlab, username, gbs, pattern)
        }
        _ => {
            unreachable!();
        }
    }
}

fn is_usize(s: String) -> Result<(), String> {
    s.as_str()
        .parse::<usize>()
        .map(|_| ())
        .map_err(|e| format!("{:?}", e))
}
