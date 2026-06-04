# BoogieBox Desktop Client

Windows desktop client for BoogieBox, built with Tauri 2 (Rust) and the existing React web UI.

## Architecture

The desktop app is a thin Tauri shell that:

- Opens the existing BoogieBox React/web UI in a WebView2 window
- Probes the configured BoogieBox server on startup
- Adds native Windows window management and config persistence
- Plays video with an embedded libmpv backend, not the browser video element
- Keeps all server logic (Express, SQLite, scanning, transcoding) untouched

## Prerequisites

- **Node.js 22+** (`node --version`)
- **Rust stable** (`rustup update stable`)
- **Microsoft C++ Build Tools** (MSVC v143 or later, via Visual Studio installer)
- **WebView2 Runtime** (pre-installed on Windows 10 1803+ and Windows 11)
- **Tauri CLI**: installed from `package.json` devDependencies
- **libmpv runtime**: `mpv-2.dll` or `libmpv-2.dll` in `src-tauri/resources/libmpv/`

## Quick Start

```bat
:: In one terminal - start the BoogieBox server
cd server
npm run dev

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
- `videoBackend` - `"embedded-libmpv"` (desktop video uses bundled libmpv)

Example:

```json
{
  "serverUrl": "http://localhost:3001",
  "videoBackend": "embedded-libmpv"
}
```

## Building a Release

```bat
cd desktop
npm install
npm run build
```

Output is placed under `src-tauri/target/release/bundle/`.

See `build-desktop.bat` at the repo root for the automated build with prerequisite checks.

## Video Playback

Desktop video is handed off from React to the Tauri backend and decoded by embedded libmpv.
The React `<video>` element is not used in the desktop shell, so playback avoids WebView2
codec/container limits.

The bundled libmpv runtime must include `mpv-2.dll` or `libmpv-2.dll` under
`src-tauri/resources/libmpv/`. Dev mode loads from that source folder directly. The Tauri
production build copies the same folder into the app resources. At runtime, BoogieBox looks next
to `BoogieBox.exe` first, then in bundled `libmpv/`, then in the source/resource roots, then on
`PATH`. It does not spawn `mpv.exe`.

## Icon Regeneration

```bat
cd desktop
npm run generate-icons
```

Replaces placeholder icons in `src-tauri/icons/` with new versions. For production, replace
with a professionally designed 1024x1024 source image before running this script.
