# Local WinSW build cache

Place the stable WinSW v2.12.0 x64 wrapper executable here when building a standalone BoogieBox server installer:

- `boogiebox-service.exe`

Download `WinSW-x64.exe` from the WinSW v2.12.0 GitHub release:

```text
https://github.com/winsw/winsw/releases/download/v2.12.0/WinSW-x64.exe
```

Verify it against the pinned SHA-256 before renaming:

```text
05b82d46ad331cc16bdc00de5c6332c1ef818df8ceefcd49c726553209b3a0da  WinSW-x64.exe
```

Rename it to `boogiebox-service.exe`, and keep its license notice with your distribution records. The standalone build copies the wrapper into the release folder so the Inno Setup installer can register `boogiebox-server.exe` as an optional Windows service. `setup-dev.bat` automates this download and checksum check.

This folder is intentionally ignored by Git except for this README.
