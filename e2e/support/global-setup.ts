/**
 * Playwright global setup: fresh run folder, fixture library, mock providers, throwaway server. Returns the teardown.
 */

import fs from 'node:fs'
import type { ChildProcess } from 'node:child_process'
import { prepareLibrary } from './library.ts'
import { BASE_URL, FIXTURE_DIR, MUSIC_DIR, RUN_DIR, resolveFfmpeg, resolveServerBuild } from './paths.ts'
import { PROVIDER_URL, startProviderMock, stopProviderMock } from './providers.ts'
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

  // The mock stands in for every online provider and must be up before the server's first lookups.
  const providers = await startProviderMock()
  let child: ChildProcess
  try {
    child = await startServer(build)
  } catch (error) {
    await stopProviderMock(providers)
    throw error
  }
  console.log(`[e2e] Server listening at ${BASE_URL} (pid ${child.pid}); mock providers at ${PROVIDER_URL}; run folder ${RUN_DIR}\n`)
  return async () => {
    stopServer(child)
    await stopProviderMock(providers)
  }
}
