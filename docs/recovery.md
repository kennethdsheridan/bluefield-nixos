# Recovery

Use RShim console and vendor BFB recovery before destructive testing.

If a completed install drops to UEFI shell, try the removable fallback first:

```text
FS0:\EFI\BOOT\BOOTAA64.EFI
```

If that boots NixOS, inspect `bootctl status` or `efibootmgr -v`. A stale EFI
entry that references an old ESP PARTUUID must be recreated against the current
ESP.
