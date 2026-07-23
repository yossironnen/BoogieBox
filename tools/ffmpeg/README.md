# Local FFmpeg build cache

Place local Windows FFmpeg binaries here when building a standalone BoogieBox server package:

- `ffmpeg.exe`
- `ffprobe.exe`
- license/notice files from the selected FFmpeg distribution (e.g. `LICENSE.txt`)

This folder is intentionally ignored by Git except for this README.

## One-time provisioning

Download a selected Windows FFmpeg build and place `ffmpeg.exe`, `ffprobe.exe`, and the matching license/notice files in this folder.

## Manual provisioning

Download the pinned FFmpeg 8.1.2 essentials build:

```text
https://www.gyan.dev/ffmpeg/builds/packages/ffmpeg-8.1.2-essentials_build.zip
```

Verify it against the published SHA-256 before extracting:

```text
db580001caa24ac104c8cb856cd113a87b0a443f7bdf47d8c12b1d740584a2ec  ffmpeg-8.1.2-essentials_build.zip
```

Extract `ffmpeg.exe` and `ffprobe.exe` here. Keep the license/notice file beside the binaries so it is included in the release package. `setup-dev.bat` automates this download and checksum check; update both the pinned version here and in `setup-dev.bat` together when moving to a newer FFmpeg release.

## Build behavior

`build-server-rust.bat` copies this entire folder into `resources/ffmpeg/` in the release package and fails clearly when either executable is missing.
