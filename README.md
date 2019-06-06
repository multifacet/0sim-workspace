# Contents

- `runner/` is a self-contained program that is capable of setting up any
  experiment for the project and running it.
    - For more info on usage: `cd runner; cargo run -- help`.
    - The code itself is also pretty well-documented IMHO.
- `bmks/` contains files needed for some benchmarks (e.g. NAS).
- `vagrant/` contains the `Vagrantfile` used for the VMs in the experiments.
- `0sim` is a git submodule to the repo with the kernel/simulator code.
    - The submodule path in the `.gitmodules` file is relative so that it can
      work from different methods of checking out (e.g. https vs git).
    - [Here is a link to the repo](https://github.com/mark-i-m/0sim)
- `0sim-experiments` is a git submodule that contains some microbenchmarks.
    - [Here is a link to the repo](https://github.com/mark-i-m/0sim-experiments)
- `0sim-trace` is a git submodule that contains the tracer.
    - [Here is a link to the repo](https://github.com/mark-i-m/0sim-trace)

# Building

The `runner` and a lot of the other things are built in rust. These can be
built and run using the standard `cargo` commands.

You can install rust via [rustup.rs](https://rustup.rs). The following versions
should work:

- `runner`: stable rust 1.34
- `0sim-trace`: stable rust 1.34
- `0sim-experiments`: nightly rust 1.37
    - we use inline `asm`, which alas is still unstable :'(

Generally, `runner` is the only one that needs to be built or run on your local
machine. These others only need to run on test machines.
