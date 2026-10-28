/**
 * UI release suite: drives a packaged BoogieBox server (Releases/boogiebox-<VERSION>-*-rs) against a generated
 * fixture library in a throwaway data folder. Run with `npm run e2e` (see e2e/README.md).
 */

import { defineConfig, devices } from '@playwright/test'
import { ADMIN_STATE, BASE_URL, RUN_DIR } from './support/paths.ts'

export default defineConfig({
  testDir: './tests',
  outputDir: `${RUN_DIR}/results`,
  globalSetup: './support/global-setup.ts',
  // One shared server and database: run serially so specs never race on library/playlist state.
  fullyParallel: false,
  workers: 1,
  forbidOnly: true,
  retries: process.env.CI ? 1 : 0,
  timeout: 60_000,
  expect: { timeout: 10_000 },
  reporter: [
    ['list'],
    ['html', { outputFolder: `${RUN_DIR}/report`, open: 'never' }],
  ],
  use: {
    baseURL: BASE_URL,
    trace: 'retain-on-failure',
    screenshot: 'only-on-failure',
    video: 'retain-on-failure',
    launchOptions: { args: ['--autoplay-policy=no-user-gesture-required'] },
  },
  projects: [
    {
      name: 'setup',
      testMatch: /first-run\.setup\.ts/,
      use: { ...devices['Desktop Chrome'], viewport: { width: 1440, height: 900 } },
    },
    {
      name: 'desktop',
      testMatch: /desktop\/.*\.spec\.ts/,
      dependencies: ['setup'],
      use: { ...devices['Desktop Chrome'], viewport: { width: 1440, height: 900 }, storageState: ADMIN_STATE },
    },
    {
      name: 'mobile',
      testMatch: /mobile\/.*\.spec\.ts/,
      dependencies: ['setup'],
      use: { ...devices['Pixel 7'], storageState: ADMIN_STATE },
    },
    {
      // Last: restoring clears every session, including the shared admin one.
      name: 'restore',
      testMatch: /restore\/.*\.spec\.ts/,
      dependencies: ['desktop', 'mobile'],
      use: { ...devices['Desktop Chrome'], viewport: { width: 1440, height: 900 }, storageState: ADMIN_STATE },
    },
  ],
})
