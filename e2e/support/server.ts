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

async function waitForStatus(child: ChildProcess, timeoutMs: number): Promise<void> {
  const deadline = Date.now() + timeoutMs
  while (Date.now() < deadline) {
    if (child.exitCode !== null) throw new Error(`Server exited early with code ${child.exitCode}; see ${LOG_DIR}`)
    try {
      const res = await fetch(`${BASE_URL}/api/system/status`)
      if (res.ok) return
    } catch {
      // not listening yet
    }
    await new Promise((resolve) => setTimeout(resolve, 250))
  }
  throw new Error(`Server did not answer ${BASE_URL}/api/system/status within ${timeoutMs} ms`)
}

export async function startServer(build: ServerBuild): Promise<ChildProcess> {
  const adjacentLocator = path.join(build.releaseDir, 'boogiebox-config.json')
  if (fs.existsSync(adjacentLocator)) {
    throw new Error(`Refusing to start: ${adjacentLocator} exists and would point the test server at a real database.`)
  }
  if (!(await portIsFree(E2E_PORT))) {
    throw new Error(`Port ${E2E_PORT} is in use. Stop whatever is using it or set BOOGIEBOX_E2E_PORT.`)
  }

  for (const dir of [SANDBOX_DIR, LOG_DIR, path.dirname(CONFIG_PATH)]) fs.mkdirSync(dir, { recursive: true })

  const env: NodeJS.ProcessEnv = {
    ...process.env,
    PORT: String(E2E_PORT),
    BOOGIEBOX_CONFIG_PATH: CONFIG_PATH,
    BOOGIEBOX_LOG_DIR: LOG_DIR,
    BOOGIEBOX_DATA_DIR: path.join(SANDBOX_DIR, 'data'),
    BOOGIEBOX_LOAD_DEV_CONFIG: '',
    // The fallback locator paths (ProgramData / ~/.config) and the suggested data folder all derive from these.
    PROGRAMDATA: SANDBOX_DIR,
    LOCALAPPDATA: SANDBOX_DIR,
    ...(IS_WINDOWS ? {} : { HOME: SANDBOX_DIR, XDG_CONFIG_HOME: path.join(SANDBOX_DIR, '.config') }),
  }
  delete env.BOOGIEBOX_CONFIG_DIR
  delete env.BOOGIEBOX_DEV_CONFIG_PATH
  delete env.BOOGIEBOX_CLIENT_BUILD_DIR

  const out = fs.openSync(path.join(LOG_DIR, 'server-stdout.log'), 'a')
  const child = spawn(build.exe, [], { cwd: RUN_DIR, env, stdio: ['ignore', out, out], windowsHide: true })
  try {
    await waitForStatus(child, 30_000)
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
