#!/usr/bin/env node
/**
 * Detects a stale npm install so build/dev scripts can reinstall before using it.
 *
 * Usage:
 *   node scripts/node-deps-stale.mjs <dir>         exit 1 (with reason) if <dir> needs `npm install`, else 0
 *   node scripts/node-deps-stale.mjs --mark <dir>  record a successful install for <dir>
 *
 * <dir> is stale when node_modules is missing, package.json / package-lock.json
 * changed since the last recorded install (e.g. after a pull), or - when the
 * package depends on Vite - Vite (with its native Rolldown binding) can't load.
 */

import { spawnSync } from 'node:child_process';
import fs from 'node:fs';
import path from 'node:path';

const STAMP = '.boogiebox-deps-stamp';

const args = process.argv.slice(2);
const mark = args[0] === '--mark';
const dir = path.resolve(mark ? args[1] ?? '.' : args[0] ?? '.');
const nodeModules = path.join(dir, 'node_modules');
const stampPath = path.join(nodeModules, STAMP);

if (mark) {
  fs.writeFileSync(stampPath, `${new Date().toISOString()}\n`);
  process.exit(0);
}

const mtime = (file) => {
  try {
    return fs.statSync(file).mtimeMs;
  } catch {
    return null;
  }
};

function staleReason() {
  if (!fs.existsSync(nodeModules)) return 'node_modules missing';
  const stamp = mtime(stampPath);
  if (stamp === null) return 'no recorded install';
  for (const manifest of ['package.json', 'package-lock.json']) {
    const changed = mtime(path.join(dir, manifest));
    if (changed !== null && changed > stamp) return `${manifest} changed`;
  }
  const pkg = JSON.parse(fs.readFileSync(path.join(dir, 'package.json'), 'utf8'));
  if (pkg.dependencies?.vite || pkg.devDependencies?.vite) {
    const probe = spawnSync(process.execPath, ['-e', "import('vite').then(() => process.exit(0), () => process.exit(1))"], {
      cwd: dir,
      stdio: 'ignore',
    });
    if (probe.status !== 0) return 'Vite native dependency missing';
  }
  return null;
}

const reason = staleReason();
if (reason) {
  console.log(`  ${path.relative(process.cwd(), dir) || '.'}: ${reason} -- reinstalling npm dependencies...`);
  process.exit(1);
}
