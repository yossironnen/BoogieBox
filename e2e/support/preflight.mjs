/**
 * Runs before `npm run e2e` (npm "pre" script): stops with install instructions when the Playwright
 * package or its Chromium browser is missing, instead of failing every test with a low-level error.
 */

import fs from 'node:fs'
import { createRequire } from 'node:module'

const require = createRequire(import.meta.url)

function fail(lines) {
  console.error(['', '[e2e] The UI release suite cannot run yet:', ...lines.map((line) => `  ${line}`), ''].join('\n'))
  process.exit(1)
}

try {
  require.resolve('@playwright/test')
} catch {
  fail(['@playwright/test is not installed.', 'Run:  npm install', 'then: npm run e2e:install'])
}

const { chromium } = await import('@playwright/test')
let executable = ''
try {
  executable = chromium.executablePath()
} catch {
  // Older/odd installs throw instead of returning a path; treat as missing.
}
if (!executable || !fs.existsSync(executable)) {
  fail([
    "Playwright's Chromium browser is not installed.",
    'Run:  npm run e2e:install',
    ...(process.platform === 'linux' ? ['On a minimal Linux server also run: sudo npx playwright install-deps chromium'] : []),
  ])
}
