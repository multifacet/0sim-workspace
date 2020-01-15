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
- See [this section](./cloudlab.md) for recommendations of CloudLab instances.

## Other requirements

- If you are using a private fork of this workspace, you will need a GitHub
  Personal Access Token to run `setup00000`, which is the main setup routine.
  [See these instructions for GitHub Tokens][pat].

- You will need access to the `multifacet/0sim-workspace` repo and its
  submodules because they will be cloned to the remote machine.

[pat]: https://help.github.com/en/articles/creating-a-personal-access-token-for-the-command-line
