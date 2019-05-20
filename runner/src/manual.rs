//! Runner for common routines like setting up an environment to manually run experiments. Note
//! that this is not setup as in "take a stock VM and install stuff" but rather "take a machine
//! with stuff installed and prepare the environment for an experiment (e.g. setting scaling
//! governor)".
//!
//! NOTE: This should not be used for real experiments. Just for testing and prototyping.

use clap::ArgMatches;

pub fn run(dry_run: bool, sub_m: &ArgMatches<'_>) -> Result<(), failure::Error> {
    // TODO
    unimplemented!();
}
