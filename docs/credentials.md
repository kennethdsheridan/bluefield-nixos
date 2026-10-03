# Credentials

Bootstrap SSH public keys are operator identity, not runtime secrets. Keep them
out of this repo and pass them from a private wrapper flake.

Enable `bluefield.credentials.requireKeys = true` for real kexec or installable
images so evaluation fails before producing an unreachable system.
