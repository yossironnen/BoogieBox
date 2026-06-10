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

Example:

```json
{
  "serverUrl": "http://localhost:3001"
}
```

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
