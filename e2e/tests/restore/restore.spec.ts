/**
 * Restore (runs last: a restore signs everyone out).
 * 1. Live restore: a playlist deleted after a backup comes back, the "restoring" state shows, and everyone signs in again.
 * 2. First-run restore on a second, brand-new server: restore that backup, point the library at this machine's
 *    music folder, sign in, and the library and playlist are there.
 */

import fs from 'node:fs'
import path from 'node:path'
import type { Page } from '@playwright/test'
import { MUSIC_DIR, RUN_DIR, resolveServerBuild } from '../../support/paths.ts'
import { extraLayout, startServer, stopServer } from '../../support/server.ts'
import { expect, main, openApp, openSettingsTab, test, uniqueName } from '../../support/test.ts'

// The restore swap answers 503 `restoring` to anything that asks meanwhile.
test.use({ allowServerErrors: true })

async function openBackup(page: Page) {
  await openSettingsTab(page, 'Advanced')
  await main(page).getByRole('navigation', { name: 'Advanced settings groups' }).getByRole('link', { name: 'Backup' }).click()
  await expect(main(page).getByText('Backups kept')).toBeVisible()
}

async function playlistNames(page: Page, baseURL?: string): Promise<string[]> {
  const res = await page.request.get(`${baseURL ?? ''}/api/playlists`)
  const body = await res.json()
  const list: Array<{ name: string }> = Array.isArray(body) ? body : body.playlists ?? []
  return list.map((p) => p.name)
}

async function signInAsAdmin(page: Page) {
  await expect(page.getByRole('heading', { name: 'Select a user' })).toBeVisible({ timeout: 20_000 })
  await page.getByRole('button', { name: 'Sign in as admin' }).click()
  await expect(page.getByRole('navigation').getByRole('button', { name: 'Home', exact: true })).toBeVisible()
}

let backupPath = ''
const playlist = uniqueName('Restore drill')

test('live restore brings back a deleted playlist and signs everyone out', async ({ page }) => {
  await openApp(page)
  await test.step('make a playlist and back it up', async () => {
    const created = await page.request.post('/api/playlists', { data: { name: playlist } })
    expect(created.ok()).toBe(true)
    await openBackup(page)
    const rows = main(page).locator('[data-backup-name]')
    const before = await rows.count()
    await main(page).getByRole('button', { name: 'Back up now' }).click()
    await expect(rows).toHaveCount(before + 1, { timeout: 30_000 })
    const overview = await (await page.request.get('/api/admin/backups')).json()
    backupPath = overview.backups[0].path
    expect(overview.backups[0].counts.playlists).toBeGreaterThan(0)
  })

  await test.step('delete it after the backup', async () => {
    const list = await (await page.request.get('/api/playlists')).json()
    const items: Array<{ id: string; name: string }> = Array.isArray(list) ? list : list.playlists ?? []
    const target = items.find((p) => p.name === playlist)!
    expect((await page.request.delete(`/api/playlists/${target.id}`)).ok()).toBe(true)
    expect(await playlistNames(page)).not.toContain(playlist)
  })

  await test.step('restore shows the restoring state, then asks to sign in', async () => {
    // Hold the answer back a little so the restoring screen is observable.
    await page.route('**/api/admin/backups/*/restore', async (route) => {
      const response = await route.fetch()
      await new Promise((resolve) => setTimeout(resolve, 1200))
      await route.fulfill({ response })
    })
    await main(page).locator('[data-backup-name]').first().getByRole('button', { name: 'Restore' }).click()
    const dialog = page.getByRole('alertdialog', { name: 'Restore this backup?' })
    await expect(dialog).toContainText('The database is intact')
    await expect(dialog).toContainText('Everyone, including you, has to sign in again.')
    await dialog.getByRole('button', { name: 'Restore' }).click()
    await expect(page.getByRole('heading', { name: 'Restoring a backup…' })).toBeVisible()
    await expect(page.getByRole('heading', { name: 'Restore complete' })).toBeVisible({ timeout: 30_000 })
    await page.getByRole('button', { name: 'Sign in' }).click()
  })

  await test.step('sign in again: the playlist is back', async () => {
    await signInAsAdmin(page)
    expect(await playlistNames(page)).toContain(playlist)
  })
})

test('first-run restore on a new server with a remapped music folder', async ({ browser }) => {
  expect(backupPath, 'the live restore test made a backup').not.toBe('')
  const root = path.join(RUN_DIR, 'second-server')
  const layout = extraLayout(root, Number(process.env.BOOGIEBOX_E2E_PORT2 || 3918))
  // The "new machine" keeps its music somewhere else.
  const movedMusic = path.join(root, 'music-here')
  fs.cpSync(MUSIC_DIR, movedMusic, { recursive: true })
  const child = await startServer(resolveServerBuild(), layout)
  const context = await browser.newContext({ baseURL: layout.baseUrl })
  const fresh = await context.newPage()
  try {
    await fresh.goto('/')
    await expect(fresh.getByRole('heading', { name: 'Set up your server' })).toBeVisible()
    await fresh.getByRole('radio', { name: /Restore from a backup/ }).check()
    const backupField = fresh.getByRole('textbox', { name: 'Backup folder' })
    await backupField.fill(backupPath)
    await backupField.blur()
    await expect(fresh.getByText('Where is your music on this machine?')).toBeVisible()
    await fresh.getByRole('textbox', { name: 'Database folder' }).fill(path.join(root, 'data'))
    // The original music folder exists here too (same disk); point it at the moved copy instead.
    const mapField = fresh.getByRole('textbox', { name: /^New folder for / })
    await mapField.fill(movedMusic)
    await fresh.getByRole('button', { name: 'Restore BoogieBox' }).click()
    await signInAsAdmin(fresh)
    expect(await playlistNames(fresh, layout.baseUrl)).toContain(playlist)
    const libraries = await (await fresh.request.get(`${layout.baseUrl}/api/libraries`)).json()
    const folders = JSON.stringify(libraries)
    expect(folders).toContain(path.basename(movedMusic))
  } finally {
    await context.close()
    stopServer(child)
  }
})
