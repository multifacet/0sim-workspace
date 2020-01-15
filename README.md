# 0sim Simulator + Tooling

This repository contains the 0sim Simulator plus a bunch of tooling that we
recommend for ergonomic use of 0sim.

While 0sim is usable on its own, it is _a lot_ more ergonomic to
use the tooling we have built around it, which is contained in this repository.
This README documents some basic workflow and features for the use of this
tooling and of 0sim.

[Jump to Getting Started](#getting-started)

## What is 0sim?

0sim simulates the behavior of system software (e.g. kernels) on huge-memory
systems (e.g. terabytes of RAM):

> Recent advances in memory technologies mean that com-
> modity machines may soon have terabytes of memory; however,
> such machines remain expensive and uncommon today. As a
> result, only a fraction of programmers and researchers can
> debug and prototype fixes for scalability problems or explore
> new system behavior caused by terabyte-scale memories.
>
> To enable rapid and early prototyping and exploration of
> system software for such machines, we built the 0sim simu-
> lator. 0sim uses virtualization to simulate the execution of
> huge workloads on modest machines. The key observation
> behind 0sim is that many workloads follow the same control
> flow regardless of their input. We call such workloads data-
> oblivious. 0sim takes advantage of data-obliviousness to make
> huge simulations feasible and fast via memory compression.

Mark Mansi and Michael M. Swift. _0sim: Preparing System Software for a World with Terabyte-scale Memories_. ASPLOS 2020.
(TODO: link)

## What is this repo?

> This section is intended to give a quick overview of how 0sim and the
> associated tools are intended to be used. Feel free to skip to [`Getting
> Started`](#getting-started) if you already know.

![Design Diagram](design.jpg)

The suggested workflow (the one we use) is to dedicate one machine to 0sim to
run experiments. One would then drive this machine remotely from some local
machine, such as a desktop workstation, via SSH.

This repository contains a few tools we have developed and found useful in
addition to 0sim. The paper (linked above mostly covers 0sim), but we rarely
interact directly with 0sim. Instead, we drive it using the tools in this
repository: `runner` and `jobserver`.

- `runner`: executes commands on the remote machine in 0sim. `runner` takes
  care of reproducible setup and execution. It also saves a ton of time. In
  general, `runner` has two types of subcommands:
    - `setup*` commands do setup/configuration work.
    - `exp*` command runs experiments.

- `jobserver`: makes it easy to run a large number of experiments on one or
  more machines with possibly different parameters, collecting the results and
  logs in one place on the local machine for processing. The `jobserver` is
  more like a bookkeeping program that just forks off instances of `runner` to
  do the real work.

<a name="reqs"></a>
# Requirements

You will need two machines:
- one to run 0sim (since 0sim is a modified Linux kernel + KVM). We call this
  machine the `remote`. You need to be able to SSH to this machine **without a
  user password or RSA key passphrase**.
- one to run the runner. This machine should have a persistent network
  connection, so we would recommend some lab machine or a desktop machine. We
  call this machine the `local`.

## Local machine requirements

- You will need stable rust on your local machine to build and run `runner`. We
use 1.35, but slightly older versions should also work, and any newer version
will work. You can get rust [here](https://rustup.rs).

- You will need an internet connection for `cargo` to download dependencies.

- You will need **passphrase-less** SSH access to the remote machine from the
  local machine. That is, you will need to use SSH, and the SSH key that must
  _not_ have a passphrase.

- `runner` should compile and run on Linux, MacOS, or Windows, but we have only
  tried Linux.

## Remote machine requirements

- The remote machine should be running CentOS 7. Newer versions may work, but we
  have not tested them.

- The remote machine should have your SSH key installed in `authorized_keys`.

- You must have **password-less** `sudo` access on the remote machine.

- The remote machine must be an Intel `x86_64` machine (AMD virtualization
  extensions are not supported yet).

- The remote machine must have an unused drive or partition that can be used
  for the swap space to back the simulator.

**Recommendations**

- 32GB RAM or more
- 1-2TB of swap space, preferably SSD
- See [this section](#cloudlab) for recommendations of CloudLab instances.

## Other requirements

- If you are using a private fork of this workspace, you will need a GitHub
  Personal Access Token to run `setup00000`, which is the main setup routine.
  [See these instructions for GitHub Tokens][pat].

- You will need access to the `multifacet/0sim-workspace` repo and its
  submodules because they will be cloned to the remote machine.

[pat]: https://help.github.com/en/articles/creating-a-personal-access-token-for-the-command-line

<a name="getting-started"></a>
# Getting Started

The workspace contains a bunch of tools, described in the contents section. The
most important are the `runner` and `0sim` itself. The runner drives
experiments and contains a library for writing and driving other experiments.
There is an emphasis on reproducibility and ergonomic usage. `jobserver` is
useful for running large numbers of experiments on one or more remotes with one
or more variations of parameters.

0. [Ensuring requirements (see above). Don't skip this or things will break.](#reqs)
0. [Cloning and building the runner](#runner)
0. [Using the runner to set up another machine with 0sim](#install-sim)
1. [Using the runner to run a simulation with 0sim](#run-exp)
2. [(Optional) Using the jobserver to run a large number of experiments with the runner](#jobserver)

<a name="runner"></a>
## Cloning and Building the runner

0. Clone this repository _to the local machine_ (not the remote, runner will do
   that automatically).

   ```
   git clone https://github.com/multifacet/0sim-workspace.git
   ```

   Note, you only need to clone this repository, _not_ all of the submodules. In
   particular, you do _not_ need to clone 0sim, which would take a long time.

1. If needed, edit the constant `RESEARCH_WORKSPACE_REPO` at the top of
   [`./runner/src/common.rs`][user]. This specifies the location and access
   method of the workspace repo (this repo). **NOTE**: this is the access
   method that is used to clone the repo on the _remote_, so make sure that it
   works there. For example, make sure that necessary private keys are
   installed there if using SSH.

    - If you are just using our public repository from github, you can leave it as is.
    - If you are using a private fork of our repository, you should change the
      constant to `GitRepo::HttpsPrivate` with the appropriate values filled in.
    - If you are using SSH to access the repo, you should change the constant to `GitRepo::Ssh`.
    - See the documentation comments on the `GitRepo` type just below the constant.

2. Build the runner. This may take a few minutes. This requires rust + cargo,
   as the runner is written in rust. You can install rust from [here](https://rustup.rs).

   ```
   cd runner
   cargo build
   ```

3. The runner is now built. The compiled binary is `./target/debug/runner`. You
   can pass the `--help` flag to see usage. It has a bunch of possible
   subcommands. They do various setup operations or run experiments.
   - **NOTE** The runner must be run from the `0sim-workspace/runner` directory;
   i.e. always run it as `./target/debug/runner <args>`

[user]: https://github.com/multifacet/0sim-workspace/blob/master/runner/src/common.rs#L29

<a name="install-sim"></a>
## Using the Runner to install 0sim on a remote machine.

0. Make sure the remote machine (the one that will run 0sim) is set up as
   described in the requirements. Specifically, you need passwordless access to
   the remote and you must have root access (since you will be installing a new
   kernel on it).

0. (Optional) If you are running on Cloudlab, the default root volume is only
   16GB, which is not enough. You can pass another volume to be formatted and
   mounted as the root volume by passing the `--home_device` flag in the
   command in the step below.

0. (Optional) 0sim requires a swap device to back its simulated memory, even
   though we likely won't write to most of it many workloads. The runner has
   two flags that allow different swap devices to be set up automatically:

   `--mapper_device` sets up a 10TB thin-provisioned device-mapper device.

   `--swap` uses the given devices for swap space.

   See the `--help` usage message for more info.

   You can skip this for now and set up a swap device later if you want.

0. (Optional) If you would like to build and install a recent kernel (see the
   `KERNEL_RECENT_TARBALL` constant in `src/common.rs`) in the target, use
   the `--guest_kernel` option. Alternately, you can install whatever kernel
   you want manually.

1. Run the following command, which will do all setup necessary, including
   cloning the workspace, compiling 0sim, and installing it. This takes about 1
   hour on our machine, but it will vary depending on how many cores the remote
   has.

   ```
   ./target/debug/runner setup00000 $ADDR $ME --host_dep --create_vm \
   --host_bmks --prepare_host --host_kernel markm_ztier --clone_wkspc \
   --guest_bmks
   # optionally (if not using public repo) --secret $TOKEN
   # optionally --home_device /dev/sdc --mapper_device /dev/sdb
   ```

   where `$ADDR` is the SSH address:port of the machine (e.g. `mark.cs.wisc.edu:22`),
   `$ME` is the username that will run the experiments, and `$TOKEN` is the
   GitHub Personal Access Token.

   There are also some additional flags you can pass (e.g. to disable EPT or
   build extra benchmarks), including the flags from the previous steps. Run
   the following command to see what they are:

   ```
   ./target/debug/runner setup00000 --help
   ```

   **NOTE** The runner must be run from the `0sim-workspace/runner` directory;
   i.e. always run it as `./target/debug/runner <args>`

<a name="run-exp"></a>
## Using the Runner to run experiments on a remote machine.

Experiment scripts are implemented as modules of the `runner` program. Each one
exports a subcommand. You can see all of the available experiments by passing
the `--help` flag to the runner:

```
./target/debug/runner --help
```

`runner` is extensible and contains a library of useful function for adding new
experiments in the form of new modules/subcommands.

`runner` also contains infrastructure for recording parameters and code
versions of the experiments to improve reproducibility.

0. Implement your experiment as a subcommand of the `runner` or choose one of
   the existing experiments.

1. Choose the parameters you want to use for experiment. Pass `--help` to the
   experiment subcommand to see available parameters.

2. Run the following command on the _local_ machine:

   ```
   ./target/debug/runner expXXXXX $ADDR $ME ARG1 ARG2...
   ```

   where $ADDR is the IP:PORT of the remote SSH server, $ME is the remote user
   that will be used to run the experiments, and ARG1, ARG2, etc are the
   arguments to the experiment. For example, one might run the following:

   ```
   ./target/debug/runner exp00000 marks-machine.cs.wisc.edu:22 markm 4096 1 -m
   ```

   **NOTE** The runner must be run from the `0sim-workspace/runner` directory;
   i.e. always run it as `./target/debug/runner <args>`

3. Wait for the experiment to terminate.

4. Most experiments output results to a directory on the remote:
   `$HOME/vm_shared/results/`. These results can then be moved to wherever they
   will be consumed. There are a few different output files for each experiment:
    - The data generated by the experiment (usually `.out`, but some experiments
      use other extensions, especially if there are multiple generated data
      files).
    - The parameters/settings of the experiment (`.params`), including the git
      hash of the workspace.
    - The time to run the experiment (`.time`).
    - Infomation about the platform and target, useful for debugging (`.sim`),
      including the output of `lscpu`, `lsblk`, and `dmesg`, memory usage, and
      zswap status.

<a name="jobserver"></a>
## Using the jobserver to run many experiments.

The jobserver exists to solve a problem that we often ran into while developing 0sim:
- One has multiple machines to run experiments on (e.g. multiple cloudlab instances).
- One has many experiments to run.
- The experiments need to be rerun with a bunch of different combinations of the parameters.

Trying to manage this all by hand is extremely tedious, time consuming, and
inefficient. The jobserver does this for you.

For more info on how to use the jobserver, please see the [jobserver
README](https://github.com/mark-i-m/jobserver).

<a name="cloudlab"></a>
# Cloudlab

## Recommended Hardware/Profile

0. The following profile allows for easy creation of one or more Centos 7 instances:
   https://www.cloudlab.us/p/SuperPages/centos-n-bare-metal

1. The `c220g2` instance type is well-suited for 0sim.
    - There are two spare drives: a smaller SSD (usually `/dev/sdc`) and a
      large HDD (usually `/dev/sdb`). Depending on how data-oblivious your
      workload is, you may want to make the HDD your swap space because it is
      larger, even though it is slower.
    - You can use the `--mapper_device`, `--swap`, and `--home_device` options
      of the `runner` with setup00000 to setup a cloudlab machine. See the
      usage message.

## Cloudlab Troubleshooting

0. On occasion, there is a Mellanox RDMA driver that conflicts with KVM/QEMU's
   install dependencies. You will get an error while running `setup00000`.
   Uninstall the Mellanox driver; we don't need it. The restart the script.

1. There is a bug in the `spurs` library that we have yet to debug. Often the
   script will crash with the unhelpful error `no other error listed`. Usually
   this happens just after we finish setting up the host and rebooting it in
   `setup00000`. In this case, just restart the script. You can use the `-v`
   flag to set up only the VM, which is what we want.

# Repository Contents

- `runner/` is a self-contained program that is capable of setting up any
  experiment for the project and running it.
    - For more info on usage:
        - `cd runner; cargo run -- help`.
        - There is a `README.md`
        - The code itself is also pretty well-documented IMHO.
- `jobserver/` is a self-contained jobserver and client. See that repo and the
  client CLI for more info.
- `bmks/` contains files needed for some benchmarks (e.g. NAS).
- `vagrant/` contains the `Vagrantfile` used for the VMs in the experiments.
- `0sim` is a git submodule to the repo with the kernel/simulator code.
    - The submodule path in the `.gitmodules` file is relative so that it can
      work from different methods of checking out (e.g. https vs git).
    - [Here is a link to the repo](https://github.com/multifacet/0sim)
- `0sim-experiments` is a git submodule that contains some microbenchmarks.
    - [Here is a link to the repo](https://github.com/multifacet/0sim-experiments)
- `0sim-trace` is a git submodule that contains the tracer.
    - [Here is a link to the repo](https://github.com/multifacet/0sim-trace)

# Licensing

0sim itself is implemented as a modified Linux kernel, so it maintains the GPL
license of the Linux kernel.

The tools in this repository are licensed under the Apache v2 open-source license.

# Troubleshooting

We will collect common issues here as they are reported...

- Vagrant error: `Call to virDomainCreateWithFlags failed: internal error: qemu
  unexpectedly closed the monitor: 2019-11-26T23:49:09.807847Z qemu-kvm:
  unrecognized feature phys-bits`
    - Cause: This happens because `libvirtd` is using an old version of QEMU
      packaged with Centos which is masking the recent version compiled from
      source by `runner`.
    - Solution: Uninstall the old version: `sudo yum remove qemu-kvm`, and
      restart libvirtd: `sudo systemctl restart libvirtd`

- Out of memory (OOM) killing: The target workload is OOM killed near the end
  by the target kernel.
    - Cause: unfortunately, the exact amount of memory used by a workload
      running on Linux and the exact amount of memory available before
      triggering the OOM killer are both difficult quantities to estimate.
      They vary from system to system.
    - Solution: sometimes rerunning the workload may be sufficient. Sometimes
      it is necessary to modify the `runner` script for the experiment to
      create a larger target or a smaller workload. Some scripts also have
      flags to modify the workload size.

- Kernel `ENOSPC` when swapping + no apparent progress.
    - Causes: if a thinly-provisioned swap space fills up, the device mapper
      system will return a `ENOSPC` error to the swapping subsystem. One can
      see this error in the kernel log on the platform (via `dmesg`).
    - Solution: You need a large physical storage device to back the
      thin-provisioned device mapper device.

- `swapon: /dev/sdk: read swap header failed: Invalid argument` or similar
  on a machine with many storage devices.
    - Causes: often the devices will move around and get different names after
      a reboot. This means that if you configured (via `setup00000`) 0sim to
      use a particular device (say `/dev/sdk`) for swap space, it might fail
      after a reboot.
    - Solution: pass the `--unstable_device_names` flag to `setup00000` along
      with your other arguments. For example:

      ```console
      --swap sda sdh sdi --unstable_device_names
      ```

      This will cause setup to use device-id based paths, which are stable.

# Known Issues

There are some issues of which we are aware but do not have a good solution.

- SSH Errors: "Broken pipe" or "Unable to create channel" or variants
    - Cause: This happens because the target gets so far behind the platform
      that the SSH connection times out. Unfortunately, this often happens for
      large simulations.
    - We also suspect that this issue is worsened when Intel EPT (nested paging)
      extensions are enabled. One can use `runner setup00000` to disable EPT
      (see usage message).

- Unable to boot target larger than 1028GB on machine without EPT on some machines.
    - Cause: We believe this is a KVM bug but are not 100% sure. The fact that
      1024-1027GB machines boot and run indicates that this is not a hardware
      limitation.

- Unable to boot large multicore target.
    - Cause: This also happens with stock KVM. We suspect this is due to some
      sort of hardware timeout when KVM emulates devices during boot, but we
      are not really sure.
    - Disabling TSC offsetting during target boot can solve this in some cases.

# List of `runner` Experiments

The `runner` has a bunch of subcommands (see `./runner help`) to do different
setup routines and run different experiments from our paper. Each one has a
submodule in the `runner` source code and command line option. This section
contains a list of the current set of sucommands and what each one does. Please
see the source code and the `./runner help` messages for more info.

Setup routines do setup/configuration tasks. They do not run any experiments,
but are required to run before experiments can run.

- `setup00000`: The main setup routine that installs 0sim.
- `setup00001`: Auxilliary setup routine that builds and installs a kernel in a
  virtual machine.

Experiments:

- `exp00000`: Runs one of the following workloads in simulation:
    - A single-threaded memcached client that does insertions on a memcached
      server on the same host.
    - A microbenchmark that mmaps and touches pages linearly in memory.
    - A microbenchmark that mmaps pages and writes a incrementing counter to
      each page.
    - A single-threaded redis client that does insertions on a redis server on
      the same host.
    - The Metis in-memory MR workload doing a matrix multiplication (this one
      tends to crash for large workloads).

- `exp00002`: Runs one of the following microbenchmarks (in simulation) that
  evaluates 0sim's TSC offsetting mechansim:
    - A workload that executes `rdtsc` repeatedly.
    - A workload that produces either very local memory accesses or one with
      very poor temporal and spatial locality.

- `exp00003`: Runs a memcached workload in simulation the presence of intense
  kernel compaction activity. This requires `setup00001` with the
  `markm_instrumented` branch.

- `exp00004`: Similar to `exp00003` but runs on bare-metal and intended as a
  comparison baseline. This requires `setup00000` run with the
  `markm_instrumented` branch.

- `exp00005`: Runs NAS CG class E in simulation and collects compressibility
  statistics.

- `exp00006`: Boot the kernel in simulation and collect metrics from struct
  page initiailization. This requires `setup00001` be run with branch
  `markm_instrument_ktask` or `markm_instrumented`.

- `exp00007`: Runs one of a few workloads in simulation and collects
  `/proc/buddyinfo` periodically.

- `exp00008`: Runs one of a few workloads in simulation and collects infomation
  about direct and indirect reclamation. This requires `setup00001` be run with
  branch `markm_instrumented`.

- `exp00009`: Runs either a microbenchmark or memcached in simulation while
  running a kernel build on the host. This is intended as a test for 0sim's TSC
  offsetting mechansim.

- `exp00010`: Runs one of a few workloads on bare-metal; intended as a
  comparison baseline.

- `exptmp`: A perpetually unstable experiment where I play around with trying
  to get things to work. Having a separate name makes it easier to not
  accidentally use the results for anything.
