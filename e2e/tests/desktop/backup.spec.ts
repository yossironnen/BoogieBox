/**
 * Settings › Advanced › Backup: back up now (with an uploaded cover), the schedule and keep settings, and delete.
 * Restoring signs everyone out, so it runs in the separate `restore` project (tests/restore/).
 */

import type { Page } from '@playwright/test'
import { ALBUMS } from '../../support/library.ts'
import { expect, main, openApp, openSettingsTab, test } from '../../support/test.ts'

/** 1×1 PNG. */
const PIXEL_PNG = 'iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8BQDwAEhQGAhKmMIQAAAABJRU5ErkJggg=='

async function openBackup(page: Page) {
  await openSettingsTab(page, 'Advanced')
  await main(page).getByRole('navigation', { name: 'Advanced settings groups' }).getByRole('link', { name: 'Backup' }).click()
  await expect(main(page).getByText('Backups kept')).toBeVisible()
}

test.beforeEach(async ({ page }) => openApp(page))

const rows = (page: Page) => main(page).locator('[data-backup-name]')

test('back up now lists the backup with its uploaded artwork, then delete removes it', async ({ page }) => {
  const album = ALBUMS[0]
  await test.step('upload a cover (uploaded artwork is always backed up)', async () => {
    const albums = await (await page.request.get('/api/albums', { params: { limit: 100 } })).json()
    const list: Array<{ id: string; title: string }> = Array.isArray(albums) ? albums : albums.albums ?? albums.items ?? []
    const target = list.find((a) => a.title === album.album)
    expect(target, `album ${album.album}`).toBeTruthy()
    const res = await page.request.post(`/api/albums/${target!.id}/artwork`, { data: { imageBase64: PIXEL_PNG, mimeType: 'image/png' } })
    expect(res.ok()).toBe(true)
  })

  await openBackup(page)
  const before = await rows(page).count()
  await main(page).getByRole('button', { name: 'Back up now' }).click()
  await expect(rows(page)).toHaveCount(before + 1, { timeout: 30_000 })
  const row = rows(page).first()
  await expect(row).toContainText('Manual')
  await expect(row).toContainText('10 tracks')
  await expect(row).not.toContainText(' 0 artwork')
  await expect(main(page).getByText('Last backup').locator('..')).toContainText('Today,')

  await row.getByRole('button', { name: /^Delete backup from / }).click()
  const confirm = page.getByRole('alertdialog', { name: 'Delete backup' })
  await confirm.getByRole('button', { name: 'Delete' }).click()
  await expect(rows(page)).toHaveCount(before)
})

test('schedule and keep settings save', async ({ page }) => {
  await openBackup(page)
  const card = main(page).locator('[data-backup-card="schedule"]')
  const schedule = card.getByRole('group', { name: 'How often' })
  await schedule.getByRole('button', { name: 'Daily' }).click()
  await expect(card.getByLabel('Day')).toHaveCount(0)
  await card.getByLabel('Keep').fill('6')
  await card.getByRole('button', { name: 'Save' }).click()
  await expect(card.getByRole('status').filter({ hasText: 'Saved' })).toBeVisible()

  await page.reload()
  await openBackup(page)
  await expect(schedule.getByRole('button', { name: 'Daily' })).toHaveAttribute('aria-pressed', 'true')
  await expect(card.getByLabel('Keep')).toHaveValue('6')
  await expect(main(page).getByText('Backups kept').locator('..')).toContainText('of 6')

  // Put the defaults back for the other specs.
  await schedule.getByRole('button', { name: 'Weekly' }).click()
  await card.getByLabel('Keep').fill('4')
  await card.getByRole('button', { name: 'Save' }).click()
  await expect(card.getByRole('status').filter({ hasText: 'Saved' })).toBeVisible()
  await expect(card.getByLabel('Day')).toHaveValue('0')
})
