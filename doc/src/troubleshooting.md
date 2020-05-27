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

- Unable to boot target larger than 511GB. `vagrant up` hangs at "Waiting for
  guest to obtain IP" or "Waiting for guest to respond SSH".
    - Cause: Often this happens on standard personal computers (as opposed to
      server-class machines) when Intel EPT is enabled. These are part of
      Intel's extended/nested paging extensions. Most non-server processors we
      have tested have 39 physical-address bits, which is enough to address
      512GB of memory. By enabling EPT, the processor is forced to translate
      addresses that are too wide. QEMU manifests this by conveniently and
      inexplicably hanging at guest boot time without any error messages.
    - Solution: Disable EPT on the platform machine. This can be done through
      the runner (`setup00000`):

      ```
      ./runner setup00000 {MACHINE} {USER} --disable_ept
      ```

- Unable to boot target larger than 1028GB on machine without EPT on some machines.
    - Cause: We believe this is a KVM bug but are not 100% sure. The fact that
      1024-1027GB machines boot and run indicates that this is not a hardware
      limitation.

- Unable to boot large multicore target.
    - Cause: This also happens with stock KVM. We suspect this is due to some
      sort of hardware timeout when KVM emulates devices during boot, but we
      are not really sure.
    - Disabling TSC offsetting during target boot can solve this in some cases.
    - In some cases, running with 1023GB instead of >=1024GB resolves the
      issue. This seems like a KVM bug.

- Hanging/Unresponsive Remote. Occasionally, the simulation platform becomes
  unresponsive. It is ok to forcibly reset the remote, even if the machine is
  booted.
    - This may cause the target's VM image to become corrupted. The image can
      be recreated by rerunning the appropriate `setup*` `runner` subcommands.
      Note that a subset of flags can be passed to `setup00000` (e.g. one does
      not need to keep passing `--host_kernel`). The `--DESTROY_EXISTING` flag
      will destroy any existing target VM.
