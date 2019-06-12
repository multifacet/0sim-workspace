//! This program runs different routines remotely. Which routine is chosen by passing different
//! command line arguments. certain routines require extra arguments.

// Useful common routines
#[macro_use]
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
mod exp00006;
mod exptmp;

fn run() -> Result<(), failure::Error> {
    let matches = clap::App::new("runner")
        .about(
            "This program runs different routines remotely. Which routine is chosen by passing \
             different command line arguments. certain routines require extra arguments.",
        )
        .arg(
            clap::Arg::with_name("DRY")
                .short("d")
                .long("dry_run")
                .help("Don't actually execute commands. Just print what would run and exit."),
        )
        .subcommand(setup00000::cli_options())
        .subcommand(setup00001::cli_options())
        .subcommand(setup00002::cli_options())
        .subcommand(manual::cli_options())
        .subcommand(exptmp::cli_options())
        .subcommand(exp00000::cli_options())
        .subcommand(exp00002::cli_options())
        .subcommand(exp00003::cli_options())
        .subcommand(exp00004::cli_options())
        .subcommand(exp00005::cli_options())
        .subcommand(exp00006::cli_options())
        .setting(clap::AppSettings::SubcommandRequired)
        .setting(clap::AppSettings::DisableVersion)
        .get_matches();

    let dry_run = matches.is_present("DRY");

    match matches.subcommand() {
        ("setup00000", Some(sub_m)) => setup00000::run(dry_run, sub_m),
        ("setup00001", Some(sub_m)) => setup00001::run(dry_run, sub_m),
        ("setup00002", Some(sub_m)) => setup00002::run(dry_run, sub_m),
        ("manual", Some(sub_m)) => manual::run(dry_run, sub_m),
        ("exptmp", Some(sub_m)) => exptmp::run(dry_run, sub_m),
        ("exp00000", Some(sub_m)) => exp00000::run(dry_run, sub_m),
        ("exp00002", Some(sub_m)) => exp00002::run(dry_run, sub_m),
        ("exp00003", Some(sub_m)) => exp00003::run(dry_run, sub_m),
        ("exp00004", Some(sub_m)) => exp00004::run(dry_run, sub_m),
        ("exp00005", Some(sub_m)) => exp00005::run(dry_run, sub_m),
        ("exp00006", Some(sub_m)) => exp00006::run(dry_run, sub_m),

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
