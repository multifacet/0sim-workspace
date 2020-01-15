# 0sim Simulator + Tooling

This repository contains the 0sim Simulator plus a bunch of tooling that we
recommend for ergonomic use of 0sim.

While 0sim is usable on its own, it is _a lot_ more ergonomic to
use the tooling we have built around it, which is contained in this repository.
This README documents some basic workflow and features for the use of this
tooling and of 0sim.

[Read the Getting Started Guide](https://multifacet.github.io/0sim-workspace)

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

## Repository Contents

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

## List of `runner` Experiments

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

## Licensing

0sim itself is implemented as a modified Linux kernel, so it maintains the GPL
license of the Linux kernel.

The tools in this repository are licensed under the Apache v2 open-source license.
