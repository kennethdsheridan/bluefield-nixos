# Recovery

Use RShim console and vendor BFB recovery before destructive testing.

This project's eMMC install path writes the eMMC user area (`/dev/mmcblk0`),
including the ESP and root filesystem. It intentionally does not manage the eMMC
boot partitions (`/dev/mmcblk0boot0` and `/dev/mmcblk0boot1`) that hold
BlueField boot firmware.

If a completed install drops to UEFI shell, try the removable fallback first:

```text
FS0:\EFI\BOOT\BOOTAA64.EFI
```

If that boots NixOS, inspect `bootctl status` or `efibootmgr -v`. A stale EFI
entry that references an old ESP PARTUUID must be recreated against the current
ESP.

Only update BlueField ATF/UEFI boot partitions when you are intentionally
following NVIDIA's boot-partition procedures. NVIDIA documents `bfrec`,
`mlxbf-bootctl`, UEFI boot entries, and the UPVS persistent variable store in
[BlueField Boot Flow and BFB Format](https://networking-docs.nvidia.com/bsp/4.16.0/appendix-bluefield-boot-flow-and-bfb-format).

For RShim console, `tmfifo_net<N>`, `bfb-install`, `/dev/rshim<N>/misc`, and
RShim ownership behavior, see NVIDIA's
[SoC Management Interface (RShim)](https://networking-docs.nvidia.com/bsp/4.16.0/soc-management-interface-rshim)
documentation.
