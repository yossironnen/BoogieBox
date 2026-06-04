# Local FFmpeg build cache

Place local Windows FFmpeg binaries here when building a standalone BoogieBox server package:

- `ffmpeg.exe`
- `ffprobe.exe`
- license/notice files from the selected FFmpeg distribution (e.g. `LICENSE.txt`)

This folder is intentionally ignored by Git except for this README.

## One-time provisioning

Download a selected Windows FFmpeg build and place `ffmpeg.exe`, `ffprobe.exe`, and the matching license/notice files in this folder.

## Manual provisioning

Download from https://www.gyan.dev/ffmpeg/builds/ and extract `ffmpeg.exe` and `ffprobe.exe` here. Keep the license/notice file beside the binaries so it is included in the release package.

## Build behavior

`build-server-rust.bat` copies this entire folder into `resources/ffmpeg/` in the release package and fails clearly when either executable is missing.
