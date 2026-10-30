/**
 * Mobile shell (phone viewport): tabs, browse drill-down, search and playback with the mini player.
 */

import { IMPORTED_TITLES, writeImportFixture } from '../../support/m3u.ts'
import { captureMediaSession, expect, expectAudioPlaying, mediaSession, pressMediaKey, test, uniqueName } from '../../support/test.ts'
import type { Page } from '@playwright/test'

const tabs = (page: Page) => page.getByRole('navigation', { name: 'Mobile tabs' })

test.beforeEach(async ({ page }) => {
  await page.goto('/')
  await expect(tabs(page)).toBeVisible()
})

test('home shows recent albums and the tab bar', async ({ page }) => {
  await expect(page.getByRole('heading', { name: 'Recently Added Music' })).toBeVisible()
  for (const tab of ['Home', 'Browse', 'Search', 'Playlists', 'Now']) {
    await expect(tabs(page).getByRole('button', { name: tab, exact: true })).toBeVisible()
  }
})

test('every tab renders', async ({ page }) => {
  for (const tab of ['Browse', 'Search', 'Playlists', 'Now', 'Home']) {
    await tabs(page).getByRole('button', { name: tab, exact: true }).click()
    await expect(page.getByRole('main')).toBeVisible()
  }
})

test('browse artist → album → play a track', async ({ page }) => {
  await tabs(page).getByRole('button', { name: 'Browse', exact: true }).click()
  await page.getByLabel('Artists').getByRole('button', { name: /^Mira Kestrel/ }).click()
  await page.getByLabel('Albums').getByRole('button', { name: /^Quiet Hours/ }).click()
  const tracks = page.getByLabel('Album tracks')
  await expect(tracks.getByRole('button', { name: /^Blue Hour Waltz/ })).toBeVisible()
  await tracks.getByRole('button', { name: /^Window Seat/ }).click()
  await expectAudioPlaying(page)
  await expect(page.getByRole('button', { name: /^Open now playing for Mira Kestrel/ })).toBeVisible()
})

test('search finds and plays a track', async ({ page }) => {
  await tabs(page).getByRole('button', { name: 'Search', exact: true }).click()
  await page.getByRole('searchbox', { name: 'Search your music library' })
    .or(page.getByRole('textbox', { name: 'Search your music library' }))
    .fill('Cardboard')
  const hit = page.getByRole('main').getByText('Cardboard Comet', { exact: true }).first()
  await expect(hit).toBeVisible()
  await hit.click()
  await expectAudioPlaying(page)
})

test('Now Playing shows the current track and transport works', async ({ page }) => {
  await tabs(page).getByRole('button', { name: 'Browse', exact: true }).click()
  await page.getByLabel('Artists').getByRole('button', { name: /^The Velvet Owls/ }).click()
  await page.getByLabel('Albums').getByRole('button', { name: /^Paper Moons/ }).click()
  await page.getByLabel('Album tracks').getByRole('button', { name: /^Cardboard Comet/ }).click()
  await expectAudioPlaying(page)
  await tabs(page).getByRole('button', { name: 'Now', exact: true }).click()
  await expect(page.getByRole('main').getByText('Cardboard Comet').first()).toBeVisible()
  await page.getByRole('main').getByRole('button', { name: 'Next track' }).click()
  await expect(page.getByRole('main').getByText('Ink Sky').first()).toBeVisible()
  await page.getByRole('main').getByRole('button', { name: 'Pause' }).click()
  await expect(page.getByRole('main').getByRole('button', { name: 'Play', exact: true })).toBeVisible()
})

test('OS media controls follow the mobile player', async ({ page }) => {
  await captureMediaSession(page)
  await page.reload()
  await tabs(page).getByRole('button', { name: 'Browse', exact: true }).click()
  await page.getByLabel('Artists').getByRole('button', { name: /^The Velvet Owls/ }).click()
  await page.getByLabel('Albums').getByRole('button', { name: /^Paper Moons/ }).click()
  await page.getByLabel('Album tracks').getByRole('button', { name: /^Cardboard Comet/ }).click()
  await expectAudioPlaying(page)
  await expect.poll(async () => (await mediaSession(page)).title).toBe('Cardboard Comet')
  await pressMediaKey(page, 'nexttrack')
  await expect.poll(async () => (await mediaSession(page)).title).toBe('Ink Sky')
  await pressMediaKey(page, 'pause')
  await expect.poll(async () => (await mediaSession(page)).playbackState).toBe('paused')
})

test('import an M3U from the Playlists tab', async ({ page }) => {
  const name = uniqueName('E2E Mobile Import')
  const file = writeImportFixture(`${name}.m3u`)
  await tabs(page).getByRole('button', { name: 'Playlists', exact: true }).click()
  const chooser = page.waitForEvent('filechooser')
  await page.getByRole('main').getByRole('button', { name: 'Import playlist' }).click()
  await (await chooser).setFiles(file)

  const sheet = page.getByRole('dialog', { name: 'Import playlist' })
  await expect(sheet.locator('[data-import-tile="matched"]')).toContainText('4')
  await expect(sheet.getByText('Not found · 1')).toBeVisible()
  await expect(sheet.getByRole('status')).toHaveText("4 tracks will be added, in the file's order")
  await sheet.getByRole('button', { name: 'Create playlist' }).click()
  await expect(sheet).toBeHidden()
  await expect(page.getByRole('status').filter({ hasText: 'Playlist imported' })).toContainText('4 tracks added. 3 lines were left out.')
  for (const title of IMPORTED_TITLES) {
    await expect(page.getByRole('main').getByText(title, { exact: true }).first()).toBeVisible()
  }

  const playlists: Array<{ id: string; name: string }> = await (await page.request.get('/api/playlists')).json()
  const created = playlists.find((p) => p.name === name)!
  expect((await page.request.delete(`/api/playlists/${created.id}`)).ok()).toBe(true)
})

test('build a smart playlist on the phone and open its rule-picked tracks', async ({ page }) => {
  const name = uniqueName('E2E Mobile Smart')
  const rockTitles = ['Cardboard Comet', 'Folded Orbit', 'Ink Sky']
  await tabs(page).getByRole('button', { name: 'Playlists', exact: true }).click()
  try {
    await page.getByRole('main').getByRole('button', { name: 'New smart playlist' }).click()
    const sheet = page.getByRole('dialog', { name: 'New smart playlist' })
    await sheet.getByLabel('Playlist name').fill(name)
    await sheet.getByRole('combobox', { name: 'Field' }).selectOption('genre')
    await sheet.getByRole('combobox', { name: 'Genre' }).fill('Rock')
    await sheet.getByRole('button', { name: 'Add rule' }).first().click()
    await sheet.getByRole('combobox', { name: 'Field' }).nth(1).selectOption('artist')
    await sheet.getByRole('combobox', { name: 'Condition' }).nth(1).selectOption('contains')
    await sheet.getByRole('combobox', { name: 'Artist name' }).fill('velvet')
    const summary = sheet.getByRole('button', { name: /Show tracks/ })
    await expect(summary).toContainText('3 tracks')
    await summary.click()
    await expect(sheet.getByRole('list', { name: 'First tracks' }).getByRole('listitem')).toHaveCount(3)
    await sheet.getByRole('button', { name: 'Create playlist' }).click()
    await expect(sheet).toBeHidden()

    await expect(page.getByRole('heading', { name })).toBeVisible()
    await expect(page.getByText('Picked by rules. Edit the rules to change the tracks.')).toBeVisible()
    for (const title of rockTitles) await expect(page.getByRole('button', { name: `Play ${title}` })).toBeVisible()
    await expect(page.getByRole('button', { name: /^Reorder / })).toHaveCount(0)
    await expect(page.getByRole('button', { name: 'Add', exact: true })).toHaveCount(0)

    await page.getByRole('button', { name: 'Rules', exact: true }).click()
    const editor = page.getByRole('dialog', { name: 'Edit rules' })
    await expect(editor.getByRole('combobox', { name: 'Genre' })).toHaveValue('Rock')
    await editor.getByRole('button', { name: 'Close rules' }).click()
    await expect(editor).toBeHidden()

    await page.getByRole('button', { name: 'Back' }).click()
    const card = page.getByRole('button', { name: new RegExp(`^${name.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')} — Smart playlist · 3 tracks · `) })
    await expect(card.getByTestId('smart-badge')).toBeVisible()
  } finally {
    const list: { id: string; name: string }[] = await (await page.request.get('/api/playlists')).json()
    for (const playlist of list.filter(p => p.name === name)) await page.request.delete(`/api/playlists/${playlist.id}`)
  }
})

test('the Now Playing Sleep tool starts and cancels a timer', async ({ page }) => {
  await tabs(page).getByRole('button', { name: 'Browse', exact: true }).click()
  await page.getByLabel('Artists').getByRole('button', { name: /^The Velvet Owls/ }).click()
  await page.getByLabel('Albums').getByRole('button', { name: /^Paper Moons/ }).click()
  await page.getByLabel('Album tracks').getByRole('button', { name: /^Cardboard Comet/ }).click()
  await expectAudioPlaying(page)
  await tabs(page).getByRole('button', { name: 'Now', exact: true }).click()
  await page.getByRole('main').getByRole('button', { name: 'Sleep', exact: true }).click()
  const sheet = page.getByRole('dialog', { name: 'Sleep timer' })
  await sheet.getByRole('button', { name: 'End of album' }).click()
  await expect(sheet).toBeHidden()
  const running = page.getByRole('main').getByRole('button', { name: 'Sleep timer: end of album' })
  await expect(running).toContainText('End')
  await running.click()
  await page.getByRole('dialog', { name: 'Sleep timer' }).getByRole('button', { name: 'Cancel timer' }).click()
  await expect(page.getByRole('main').getByRole('button', { name: 'Sleep', exact: true })).toBeVisible()
})
