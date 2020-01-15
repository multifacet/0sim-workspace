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
