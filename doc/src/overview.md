# Overview

> This section is intended to give a quick overview of how 0sim and the
> associated tools are intended to be used. Feel free to skip to [`Getting
> Started`](./getting-started.md) if you already know.

![Design Diagram](./design.jpg)

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
