#!/usr/bin/env node
/**
 * Reads the canonical version from the root VERSION file and propagates it
 * to every location in the project that embeds the version number.
 *
 * Locations updated:
 *   client/src/version.ts                   — APP_VERSION constant
 *   desktop/package.json                    — version field
 *   desktop/src-tauri/tauri.conf.json       — version field
 *   desktop/src-tauri/Cargo.toml            — version field
 *   server-rs/Cargo.toml                    — [workspace.package] version field
 */

import { readFileSync, writeFileSync } from 'fs';
import { resolve, dirname } from 'path';
import { fileURLToPath } from 'url';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const version = readFileSync(resolve(root, 'VERSION'), 'utf8').trim();

if (!/^\d+\.\d+\.\d+$/.test(version)) {
  console.error(`[sync-version] Invalid version in VERSION file: "${version}"`);
  process.exit(1);
}

console.log(`[sync-version] Syncing version ${version} to all locations...`);

function patchFile(relPath, patchFn) {
  const absPath = resolve(root, relPath);
  const original = readFileSync(absPath, 'utf8');
  const patched = patchFn(original);
  if (patched === original) {
    console.log(`  (unchanged) ${relPath}`);
    return;
  }
  writeFileSync(absPath, patched, 'utf8');
  console.log(`  (updated)   ${relPath}`);
}

// client/src/version.ts
patchFile('client/src/version.ts', (s) =>
  s.replace(/APP_VERSION\s*=\s*'[^']+'/, `APP_VERSION = '${version}'`)
);

// desktop/package.json
patchFile('desktop/package.json', (s) => {
  const obj = JSON.parse(s);
  obj.version = version;
  return JSON.stringify(obj, null, 2) + '\n';
});

// desktop/src-tauri/tauri.conf.json
patchFile('desktop/src-tauri/tauri.conf.json', (s) => {
  const obj = JSON.parse(s);
  obj.version = version;
  return JSON.stringify(obj, null, 2) + '\n';
});

// desktop/src-tauri/Cargo.toml  — version = "x.y.z" in [package] section
patchFile('desktop/src-tauri/Cargo.toml', (s) =>
  s.replace(/^(version\s*=\s*)"[^"]+"/m, `$1"${version}"`)
);

// server-rs/Cargo.toml  — version under [workspace.package]
patchFile('server-rs/Cargo.toml', (s) =>
  s.replace(/(^\[workspace\.package\][^\[]*version\s*=\s*)"[^"]+"/m, `$1"${version}"`)
);

console.log('[sync-version] Done.');
