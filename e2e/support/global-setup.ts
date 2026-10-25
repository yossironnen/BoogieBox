/**
 * Playwright global setup: fresh run folder, fixture library, throwaway server. Returns the teardown.
 */

import fs from 'node:fs'
import { prepareLibrary } from './library.ts'
import { BASE_URL, FIXTURE_DIR, MUSIC_DIR, RUN_DIR, resolveFfmpeg, resolveServerBuild } from './paths.ts'
import { startServer, stopServer } from './server.ts'

export default async function globalSetup(): Promise<() => Promise<void>> {
  const build = resolveServerBuild()
  if (!build.matchesVersion) {
    console.warn(`\n[e2e] WARNING: no build for the current VERSION; testing ${build.version} from ${build.releaseDir}\n`)
  } else {
    console.log(`\n[e2e] Server under test: ${build.exe}`)
  }

  fs.rmSync(RUN_DIR, { recursive: true, force: true })
  fs.mkdirSync(RUN_DIR, { recursive: true })
  prepareLibrary(resolveFfmpeg(build.releaseDir), FIXTURE_DIR, MUSIC_DIR)

  const child = await startServer(build)
  console.log(`[e2e] Server listening at ${BASE_URL} (pid ${child.pid}); run folder ${RUN_DIR}\n`)
  return async () => stopServer(child)
}
