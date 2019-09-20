# `jobserver`

This is a job server and client for running many experiments across many test
machines. In some sense, it is like a simple cluster manager.

The high-level idea is that you have a server (the `jobserver`) that runs on
your machine (some machine that is always on, like a desktop), perhaps in a
screen session or something. You have a bunch of experiments that you want to
run from some driver application or script. You also have a bunch of machines,
possibly of different types, where you want to run jobs. `jobserver` schedules
those jobs to run on those machines and copies the results back to the host
machine (the one running the server). One interracts with the server using the
stateless CLI client.

Additionally, `jobserver` supports the following:
- An awesome CLI client.
- Machine classes: similar machines are added to the same class, and jobs are
  scheduled to run on any machine in the class.
- Machine setup: the `jobserver` can run a sequence of setup machines and
  automatically add machines to its class when finished.
- Job matrices: Run jobs with different combinations of parameters.
- Automatically copies results back to the host, as long as the experiment
  driver outputs a line to `stdout` with format `RESULTS: <OUTPUT FILENAME>`.
- Good server logging.

# Building

Requires:
- `rust 1.37+`

```console
> cargo build
```

The debug build seems to be fast enough for ordinary use.

# Usage

Running the server:

```console
> cargo run --bin server -- /path/to/experiment/driver
```

You may want to run this in a `screen` or `tmux` session or something.

Running the client:

```console
> cargo run --bin client -- <args>
```

There are a lot of commands. They are well-documented by the CLI usage message.

I recommend creating an alias for this client. I use the alias `j`, so that I
can just run commands like:

```console
> j job ls
```
