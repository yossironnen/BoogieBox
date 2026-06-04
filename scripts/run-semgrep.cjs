/**
 * Defines a repository script for Run Semgrep.
 */

const { spawnSync } = require('node:child_process')
const fs = require('node:fs')
const path = require('node:path')

const audit = process.argv.includes('--audit')
const configs = ['.semgrep.yml', 'p/typescript', 'p/expressjs']
const args = [
  'scan',
  '--metrics=off',
  '--timeout=30',
  '--no-git-ignore',
  '--exclude',
  'node_modules',
  '--exclude',
  'client/node_modules',
  '--exclude',
  'server/node_modules',
  '--exclude',
  'desktop/node_modules',
  '--exclude',
  'server-rs/target',
  '--exclude',
  'desktop/src-tauri/target',
  ...configs.flatMap((config) => ['--config', config]),
  ...(audit ? [] : ['--error']),
  'client/src',
  'server-rs',
  'scripts',
]

function resolveSemgrepCommand() {
  if (process.platform !== 'win32') return 'semgrep'

  const semgrepOnPath = spawnSync('where.exe', ['semgrep'], { encoding: 'utf8' })
  if (semgrepOnPath.status === 0) {
    const firstMatch = semgrepOnPath.stdout.split(/\r?\n/).map((line) => line.trim()).find(Boolean)
    if (firstMatch) return firstMatch
  }

  const localAppData = process.env.LOCALAPPDATA
  const appData = process.env.APPDATA
  const userProfile = process.env.USERPROFILE
  const candidates = [
    localAppData && path.join(localAppData, 'Programs', 'Python', 'Python314', 'Scripts', 'semgrep.exe'),
    localAppData && path.join(localAppData, 'Programs', 'Python', 'Python313', 'Scripts', 'semgrep.exe'),
    localAppData && path.join(localAppData, 'Programs', 'Python', 'Python312', 'Scripts', 'semgrep.exe'),
    appData && path.join(appData, 'Python', 'Python314', 'Scripts', 'semgrep.exe'),
    appData && path.join(appData, 'Python', 'Python313', 'Scripts', 'semgrep.exe'),
    appData && path.join(appData, 'Python', 'Python312', 'Scripts', 'semgrep.exe'),
    userProfile && path.join(userProfile, '.local', 'bin', 'semgrep.exe'),
  ].filter(Boolean)

  return candidates.find((candidate) => fs.existsSync(candidate)) || 'semgrep'
}

const semgrepCommand = resolveSemgrepCommand()
const result = spawnSync(semgrepCommand, args, { stdio: 'inherit', shell: false })

if (result.error && result.error.code === 'ENOENT') {
  console.error('[ERROR] Semgrep CLI is required for this local build check. Install Semgrep and ensure semgrep is on PATH.')
  process.exit(1)
}

if (result.error) {
  console.error(`[ERROR] Failed to start Semgrep: ${result.error.message}`)
  process.exit(1)
}

process.exit(result.status ?? 1)
