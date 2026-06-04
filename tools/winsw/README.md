# Local WinSW build cache

Place the stable WinSW v2.12.0 x64 wrapper executable here when building a standalone BoogieBox server installer:

- `boogiebox-service.exe`

Download `WinSW-x64.exe` from the WinSW v2.12.0 GitHub release:

```text
https://github.com/winsw/winsw/releases/download/v2.12.0/WinSW-x64.exe
```

Rename it to `boogiebox-service.exe`, and keep its license notice with your distribution records. The standalone build copies the wrapper into the release folder so the Inno Setup installer can register `boogiebox-server.exe` as an optional Windows service.

This folder is intentionally ignored by Git except for this README.
