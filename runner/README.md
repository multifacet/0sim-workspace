# `runner`

This program can run any of the experiments.

## Usage

The runner is a CLI tool that has multiple subcommands, one for each possible
experiment or setup script. You can use `cargo run -- help` to see the possible
subcommands, each of which also has a `--help` flag. Each subcommand is
implemented in a file of the same name, and the comments at the top of those
files explain what the command does. (The code is also well-documented and
straightforward IMHO).

`runner` runs on a local machine (e.g. your laptop or workstation), and it sets
up or runs an experiment on a _remote_ machine (e.g. cloudlab). All of this is
done via SSH.

## Building

### Local machine requirements

- You will need stable rust on your local machine to build and run `runner`. I
use 1.35, but slightly older versions should also work, and any newer version
will work. You can get rust [here](https://rustup.rs).

- You will need an internet connection for `cargo` to download dependencies.

- You will need _passphrase-less_ SSH access to the remote machine from the
  local machine.

- `runner` should compile and run on Linux, MacOS, or Windows, but I have only
  tried Linux.

### Remote machine requirements

- The remote machine should be running CentOS 7.

- The remote machine should have your SSH key installed in `authorized_keys`.

- You must have password-less `sudo` access on the remote machine.

- The remote machine must be an `x86_64` machine.

- The remote machine must have an unused drive or partition that can be used
  for the swap space to back the simulator.

Cloudlab machines satisfy all of these properties.

### Other requirements

- You will need a [GitHub Personal Access Token][pat] to run `setup00000`,
  which is the main setup routine.

- You will need access to the `mark-i-m/research-workspace` repo and its
  submodules because they will be cloned to the remote machine.

[pat]: https://help.github.com/en/articles/creating-a-personal-access-token-for-the-command-line

## Cloudlab tips

These are suggestions; use as needed.

1. Use the `c220g2` instance type. This has two spare drives: a SSD (usually
   `/dev/sdc`) and a HDD (usually `/dev/sdb`). Use the SSD for the `mapper
   device`, which is a thinly-provisioned swap space to back the simulator. Use
   the HDD for the home device, which is formatted and used as the home
   directory (since Cloudlab machines by default only have a 16GB root volume).

2. On occasion, there is a Mellanox RDMA driver that conflicts with KVM/QEMU's
   install dependencies. You will get an error while running `setup00000`.
   Uninstall the Mellanox driver; we don't need it. The restart the script.

3. There is a bug in the `spurs` library that I have yet to debug. Often the
   script will crash with the unhelpful error `no other error listed`. Usually
   this happens just after we finish setting up the host and rebooting it in
   `setup00000`. In this case, just restart the script. You can use the `-v`
   flag to set up only the VM, which is what we want.
