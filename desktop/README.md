# BoogieBox Desktop Client

Windows desktop client for BoogieBox, built with Tauri 2 (Rust) and the existing React web UI.

## Architecture

The desktop app is a thin Tauri shell that:

- Opens the existing BoogieBox React/web UI in a WebView2 window
- Probes the configured BoogieBox server on startup
- Adds native Windows window management and config persistence
- Discovers BoogieBox servers on the local network automatically
- Keeps all server logic (Rust, SQLite, scanning, transcoding) untouched

## Prerequisites

- **Node.js 22+** (`node --version`)
- **Rust stable** (`rustup update stable`)
- **Microsoft C++ Build Tools** (MSVC v143 or later, via Visual Studio installer)
- **WebView2 Runtime** (pre-installed on Windows 10 1803+ and Windows 11)
- **Tauri CLI**: installed from `package.json` devDependencies

## Quick Start

```bat
:: In one terminal - start the BoogieBox server (from the repo root)
dev.bat

:: In another terminal - start the desktop shell
cd desktop
npm install
npm run dev
```

The desktop window opens, probes the saved/default server URL, and scans the local IPv4 network
for running BoogieBox servers on port `3001`. If it finds servers, it lists them on the connection
screen; you can also enter a server URL manually.

## Configuration

Desktop settings are persisted in:

```text
%APPDATA%\BoogieBox\desktop-config.json
```

Fields:

- `serverUrl` - BoogieBox server base URL (default: `http://localhost:3001`)
- `audio` - output settings for the Hi-Res audio engine (set in Settings › User Settings › Audio output):
  - `deviceId` - output device id; `null` follows the Windows default device
  - `mode` - `exclusive` (default) or `shared`
  - `bufferMs` - 20 (Low), 50 (Normal, default) or 150 (Safe); clamped to 20–200
  - `nearestRate` - when the DAC can't play a file's rate, resample to the nearest rate it takes (default `true`);
    `false` plays such files in shared mode
  - `pure` - Pure mode (EQ, volume levelling and crossfade off); `null` means on in Exclusive mode, off in Shared

Example:

```json
{
  "serverUrl": "http://localhost:3001",
  "audio": {
    "deviceId": null,
    "mode": "exclusive",
    "bufferMs": 50,
    "nearestRate": true,
    "pure": null
  }
}
```

## Hi-Res audio engine

In the desktop app, the "This PC (<machine name>)" output plays through a native engine
(`src-tauri/crates/boogiebox-audio`) instead of the WebView's `<audio>` element:

- Asks the server for a link per track (`POST /api/native/stream-tokens`) and reads the original file from
  `/media/n/{token}` with `Range` requests. Opus and WMA are decoded on the server to 24-bit WAV; internet radio
  goes through the server's relay.
- Decodes FLAC, ALAC, WAV, AIFF, MP3 and AAC locally and plays through WASAPI.
- **Exclusive mode** opens the device at each file's own rate and bit depth. At 100% volume with Pure mode on,
  playback is bit-perfect. Other apps are silent on that device while BoogieBox plays.
- Exclusive mode needs Windows to allow it: Sound settings › the device › Advanced › "Allow applications to take
  exclusive control". When it's off, or another app holds the device, the engine plays in shared mode and the
  dock says so.
- **Shared mode** goes through the Windows mixer; files are resampled to the mixer rate.
- Same-format tracks join gaplessly. A rate change in exclusive mode reopens the device (a short pause while
  the DAC re-locks) and skips the crossfade.
- With Pure mode off, the engine applies the EQ, volume levelling and crossfade itself (equal-power fade, TPDF
  dither when reducing bit depth or volume).
- Windows media controls, the taskbar thumbnail buttons and taskbar progress follow the engine.
- If the device disappears it pauses, and resumes within 30 seconds if the device comes back.

## Building a Release

```bat
build-desktop.bat
```

Or build without the installer:

```bat
build-desktop.bat --no-installer
```

Output is placed under `Releases\boogiebox-desktop-<version>-win\`.

See `build-desktop.bat` at the repo root for the automated build with prerequisite checks.

## Icon Regeneration

```bat
cd desktop
npm run generate-icons
```

Replaces placeholder icons in `src-tauri/icons/` with new versions. For production, replace
with a professionally designed 1024x1024 source image before running this script.
