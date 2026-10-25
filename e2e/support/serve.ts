/**
 * `npm run e2e:serve`: starts the same sandboxed server + fixture library the suite uses and keeps it running,
 * for writing or debugging tests by hand. Ctrl+C stops it.
 */

import globalSetup from './global-setup.ts'

const teardown = await globalSetup()
console.log('[e2e] Sandbox server running. Press Ctrl+C to stop.')
const stop = async () => {
  await teardown()
  process.exit(0)
}
process.on('SIGINT', stop)
process.on('SIGTERM', stop)
setInterval(() => {}, 1 << 30)
