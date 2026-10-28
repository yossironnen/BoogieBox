/**
 * Starts a throwaway BoogieBox server for the UI release suite.
 * Every location the server could read a locator or data folder from is pointed into tmp/e2e-run,
 * so a run never opens a real install's database.
 */

import { spawn, type ChildProcess } from 'node:child_process'
import fs from 'node:fs'
import net from 'node:net'
import path from 'node:path'
import {
  BASE_URL, CONFIG_PATH, E2E_PORT, IS_WINDOWS, LOG_DIR, RUN_DIR, SANDBOX_DIR, type ServerBuild,
} from './paths.ts'

function portIsFree(port: number): Promise<boolean> {
  return new Promise((resolve) => {
    const probe = net.createServer()
    probe.once('error', () => resolve(false))
    probe.once('listening', () => probe.close(() => resolve(true)))
    probe.listen(port, '0.0.0.0')
  })
}

/** Where a server keeps its locator, sandbox and logs. The suite's main server uses the run folder's. */
export type ServerLayout = { port: number; baseUrl: string; sandboxDir: string; logDir: string; configPath: string }

export const MAIN_LAYOUT: ServerLayout = {
  port: E2E_PORT, baseUrl: BASE_URL, sandboxDir: SANDBOX_DIR, logDir: LOG_DIR, configPath: CONFIG_PATH,
}

/** A second, independent server (fresh install) under `root`. */
export function extraLayout(root: string, port: number): ServerLayout {
  return {
    port,
    baseUrl: `http://127.0.0.1:${port}`,
    sandboxDir: path.join(root, 'sandbox'),
    logDir: path.join(root, 'logs'),
    configPath: path.join(root, 'config', 'boogiebox-config.json'),
  }
}

async function waitForStatus(child: ChildProcess, timeoutMs: number, layout: ServerLayout): Promise<void> {
  const deadline = Date.now() + timeoutMs
  while (Date.now() < deadline) {
    if (child.exitCode !== null) throw new Error(`Server exited early with code ${child.exitCode}; see ${layout.logDir}`)
    try {
      const res = await fetch(`${layout.baseUrl}/api/system/status`)
      if (res.ok) return
    } catch {
      // not listening yet
    }
    await new Promise((resolve) => setTimeout(resolve, 250))
  }
  throw new Error(`Server did not answer ${layout.baseUrl}/api/system/status within ${timeoutMs} ms`)
}

export async function startServer(build: ServerBuild, layout: ServerLayout = MAIN_LAYOUT): Promise<ChildProcess> {
  const adjacentLocator = path.join(build.releaseDir, 'boogiebox-config.json')
  if (fs.existsSync(adjacentLocator)) {
    throw new Error(`Refusing to start: ${adjacentLocator} exists and would point the test server at a real database.`)
  }
  if (!(await portIsFree(layout.port))) {
    throw new Error(`Port ${layout.port} is in use. Stop whatever is using it or set BOOGIEBOX_E2E_PORT.`)
  }

  for (const dir of [layout.sandboxDir, layout.logDir, path.dirname(layout.configPath)]) fs.mkdirSync(dir, { recursive: true })

  const env: NodeJS.ProcessEnv = {
    ...process.env,
    PORT: String(layout.port),
    BOOGIEBOX_CONFIG_PATH: layout.configPath,
    BOOGIEBOX_LOG_DIR: layout.logDir,
    BOOGIEBOX_DATA_DIR: path.join(layout.sandboxDir, 'data'),
    BOOGIEBOX_LOAD_DEV_CONFIG: '',
    // Lets the 10-track fixture library trip the scan's mass-removal brake (library.spec.ts).
    BOOGIEBOX_PRUNE_BRAKE_MIN: process.env.BOOGIEBOX_PRUNE_BRAKE_MIN ?? '2',
    // The fallback locator paths (ProgramData / ~/.config) and the suggested data folder all derive from these.
    PROGRAMDATA: layout.sandboxDir,
    LOCALAPPDATA: layout.sandboxDir,
    ...(IS_WINDOWS ? {} : { HOME: layout.sandboxDir, XDG_CONFIG_HOME: path.join(layout.sandboxDir, '.config') }),
  }
  delete env.BOOGIEBOX_CONFIG_DIR
  delete env.BOOGIEBOX_DEV_CONFIG_PATH
  delete env.BOOGIEBOX_CLIENT_BUILD_DIR

  const out = fs.openSync(path.join(layout.logDir, 'server-stdout.log'), 'a')
  const child = spawn(build.exe, [], { cwd: RUN_DIR, env, stdio: ['ignore', out, out], windowsHide: true })
  try {
    await waitForStatus(child, 30_000, layout)
  } catch (error) {
    stopServer(child)
    throw error
  }
  return child
}

export function stopServer(child: ChildProcess | undefined): void {
  if (!child || child.exitCode !== null) return
  child.kill()
}
