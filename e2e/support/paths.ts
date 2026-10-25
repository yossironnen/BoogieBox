/**
 * Resolves the server build, FFmpeg and scratch folders used by the UI release suite.
 */

import fs from 'node:fs'
import path from 'node:path'

export const REPO_ROOT = path.resolve(import.meta.dirname, '..', '..')
export const IS_WINDOWS = process.platform === 'win32'
export const E2E_PORT = Number(process.env.BOOGIEBOX_E2E_PORT || 3917)
export const BASE_URL = `http://127.0.0.1:${E2E_PORT}`

/** Everything a run writes lives under tmp/ (gitignored); the run folder is wiped at the start of each run. */
export const RUN_DIR = path.join(REPO_ROOT, 'tmp', 'e2e-run')
export const FIXTURE_DIR = path.join(REPO_ROOT, 'tmp', 'e2e-fixtures')
export const DATA_DIR = path.join(RUN_DIR, 'data')
export const MUSIC_DIR = path.join(RUN_DIR, 'music')
export const SANDBOX_DIR = path.join(RUN_DIR, 'sandbox')
export const LOG_DIR = path.join(RUN_DIR, 'logs')
export const CONFIG_PATH = path.join(RUN_DIR, 'config', 'boogiebox-config.json')
export const ADMIN_STATE = path.join(RUN_DIR, 'admin-state.json')

const EXE_NAME = IS_WINDOWS ? 'boogiebox-server.exe' : 'boogiebox-server'
const RELEASE_SUFFIX = IS_WINDOWS ? 'win-rs' : 'linux-rs'

function compareVersions(a: string, b: string): number {
  const pa = a.split('.').map(Number)
  const pb = b.split('.').map(Number)
  for (let i = 0; i < 3; i += 1) {
    if ((pa[i] ?? 0) !== (pb[i] ?? 0)) return (pa[i] ?? 0) - (pb[i] ?? 0)
  }
  return 0
}

export function appVersion(): string {
  return fs.readFileSync(path.join(REPO_ROOT, 'VERSION'), 'utf8').trim()
}

export type ServerBuild = { exe: string; releaseDir: string; version: string | null; matchesVersion: boolean }

/**
 * Picks the server under test: BOOGIEBOX_E2E_SERVER_EXE, else the Releases folder for the current VERSION,
 * else (unless BOOGIEBOX_E2E_STRICT_VERSION=1) the newest release folder present.
 */
export function resolveServerBuild(): ServerBuild {
  const explicit = process.env.BOOGIEBOX_E2E_SERVER_EXE?.trim()
  if (explicit) {
    const exe = path.resolve(explicit)
    if (!fs.existsSync(exe)) throw new Error(`BOOGIEBOX_E2E_SERVER_EXE does not exist: ${exe}`)
    return { exe, releaseDir: path.dirname(exe), version: null, matchesVersion: true }
  }

  const version = appVersion()
  const releasesDir = path.join(REPO_ROOT, 'Releases')
  const wanted = path.join(releasesDir, `boogiebox-${version}-${RELEASE_SUFFIX}`, EXE_NAME)
  if (fs.existsSync(wanted)) {
    return { exe: wanted, releaseDir: path.dirname(wanted), version, matchesVersion: true }
  }

  const pattern = new RegExp(`^boogiebox-(\\d+\\.\\d+\\.\\d+)-${RELEASE_SUFFIX}$`)
  const candidates = (fs.existsSync(releasesDir) ? fs.readdirSync(releasesDir) : [])
    .map((name) => ({ name, match: pattern.exec(name) }))
    .filter((entry) => entry.match && fs.existsSync(path.join(releasesDir, entry.name, EXE_NAME)))
    .map((entry) => ({ name: entry.name, version: entry.match![1] }))
    .sort((a, b) => compareVersions(b.version, a.version))

  const strict = process.env.BOOGIEBOX_E2E_STRICT_VERSION === '1'
  if (strict || candidates.length === 0) {
    const script = IS_WINDOWS ? 'build-server-rust.bat --no-test --no-installer' : './build-server-rust.sh --no-test --no-installer'
    throw new Error(`Missing server build for ${version}: ${wanted}\nRun ${script} first.`)
  }
  const latest = candidates[0]
  const releaseDir = path.join(releasesDir, latest.name)
  return { exe: path.join(releaseDir, EXE_NAME), releaseDir, version: latest.version, matchesVersion: false }
}

/** FFmpeg used to generate the fixture library: the release's bundled copy, then tools/, then PATH. */
export function resolveFfmpeg(releaseDir?: string): string {
  const exe = IS_WINDOWS ? 'ffmpeg.exe' : 'ffmpeg'
  const candidates = [
    process.env.BOOGIEBOX_FFMPEG_DIR ? path.join(process.env.BOOGIEBOX_FFMPEG_DIR, exe) : null,
    releaseDir ? path.join(releaseDir, 'resources', 'ffmpeg', exe) : null,
    path.join(REPO_ROOT, 'tools', IS_WINDOWS ? 'ffmpeg' : 'ffmpeg-linux', exe),
  ].filter((candidate): candidate is string => Boolean(candidate))
  return candidates.find((candidate) => fs.existsSync(candidate)) ?? exe
}
