/**
 * App shell: every top-level view and Settings tab renders without errors, and the build reports the
 * expected version.
 */

import { appVersion } from '../../support/paths.ts'
import { expect, goTo, main, openApp, openSettingsTab, test } from '../../support/test.ts'

test.beforeEach(async ({ page }) => openApp(page))

test('sidebar shows the version the server reports', async ({ page, request }) => {
  const status = await (await request.get('/api/system/status')).json()
  await expect(page.getByText(`v${status.version}`, { exact: true })).toBeVisible()
  if (process.env.BOOGIEBOX_E2E_SERVER_EXE === undefined && status.version !== appVersion()) {
    test.info().annotations.push({ type: 'warning', description: `Testing ${status.version}; VERSION is ${appVersion()}` })
  }
})

const VIEWS = [
  { view: 'Home', expectText: 'Recent Albums' },
  { view: 'Search', expectRole: ['textbox', 'Search titles, artists, albums...'] },
  { view: 'Browse Music', expectRole: ['group', 'Browse by'] },
  { view: 'Playlists', expectRole: ['textbox', 'Search playlists'] },
  { view: 'Mixes', expectRole: ['textbox', 'Search mixes or playlists…'] },
  { view: 'Activity', expectRole: ['tablist', 'Filter activity'] },
] as const

for (const entry of VIEWS) {
  test(`${entry.view} view renders`, async ({ page }) => {
    await goTo(page, entry.view)
    if ('expectText' in entry) await expect(main(page).getByText(entry.expectText).first()).toBeVisible()
    else await expect(main(page).getByRole(entry.expectRole[0], { name: entry.expectRole[1] })).toBeVisible()
  })
}

for (const tab of ['User Settings', 'Libraries', 'Auto-Scan', 'Integrations', 'Advanced', 'Users', 'About']) {
  test(`Settings → ${tab} renders`, async ({ page }) => {
    await openSettingsTab(page, tab)
    await expect(page.getByRole('tab', { name: tab, exact: true })).toHaveAttribute('aria-selected', 'true')
  })
}

test('sidebar can collapse and expand', async ({ page }) => {
  await page.getByRole('button', { name: 'Collapse left menu' }).click()
  await page.getByRole('button', { name: /Expand left menu/ }).click()
  await expect(page.getByRole('button', { name: 'Collapse left menu' })).toBeVisible()
})

test('library totals and status indicators are shown', async ({ page }) => {
  await expect(page.getByRole('group', { name: /^Library totals: 10 tracks, 3 artists, 4 albums/ })).toBeVisible()
  await expect(page.getByRole('img', { name: 'Library scan: Idle' })).toBeVisible()
  await expect(page.getByRole('button', { name: /^Activity · / })).toBeVisible()
})
