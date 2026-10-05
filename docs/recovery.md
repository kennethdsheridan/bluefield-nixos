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

## Wedged RShim Recovery

Stop streaming BFBs if `/dev/rshim0/misc` does not report a populated `OPN_STR`
and the DPU is not reachable over tmfifo. In lab testing an `OPN_STR N/A` state
survived RShim restart, RShim `SW_RESET`, PCI FLR, PCI bus reset, `devlink`
driver reload, and `devlink fw_activate`. Repeating `bfb-install` did not recover
the Arm side and can leave stale writers on `/dev/rshim0/boot` if the caller does
not clean up the whole process group.

First capture the state:

```bash
sudo cat /dev/rshim0/misc
ip -br addr show tmfifo_net0
ssh -o BatchMode=yes -o ConnectTimeout=5 root@192.168.100.2 hostname
```

Repair only the host-side tmfifo address if it is missing:

```bash
nix run .#bluefield-repair-host-tmfifo -- \
  --interface tmfifo_net0 \
  --address 192.168.100.1/30
```

Then try the least invasive in-band recovery steps:

```bash
# Ask the DPU to return to eMMC boot, then reset through RShim.
printf 'BOOT_MODE 1\nSW_RESET 1\n' | sudo tee /dev/rshim0/misc >/dev/null
sleep 30
sudo cat /dev/rshim0/misc

# If RShim itself is stale, restart the userspace backend and check again.
pid=$(ps -ef | awk '/[r]shim --foreground/ {print $2; exit}')
test -n "$pid" && sudo kill -9 "$pid"
sudo sh -c 'nohup rshim --foreground >/tmp/rshim-recovery.log 2>&1 &'
sleep 10
sudo cat /dev/rshim0/misc
```

If `OPN_STR N/A` persists, do not stream another experimental BFB. Move to a
platform reset that power-cycles or reboots the host/slot, then verify eMMC boot:

```bash
nix run .#bluefield-repair-host-tmfifo -- \
  --interface tmfifo_net0 \
  --address 192.168.100.1/30

ping -c 3 192.168.100.2
ssh root@192.168.100.2 'hostname; uname -r'
```

After recovery, inspect the failed BFB offline with `mlx-mkbfb -d` before another
boot attempt. Avoid BFBs that contain unexpected firmware or UEFI capsules unless
you are intentionally following NVIDIA firmware update procedures.
