# Getting Started

The workspace contains a bunch of tools, described in the contents section. The
most important are the `runner` and `0sim` itself. The runner drives
experiments and contains a library for writing and driving other experiments.
There is an emphasis on reproducibility and ergonomic usage. `jobserver` is
useful for running large numbers of experiments on one or more remotes with one
or more variations of parameters.

0. [Ensuring requirements (see above). Don't skip this or things will break.](./requirements.md)
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
   --host_bmks --prepare_host --host_kernel master --clone_wkspc \
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
