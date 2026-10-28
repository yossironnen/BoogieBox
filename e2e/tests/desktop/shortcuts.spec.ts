/**
 * Desktop keyboard shortcuts: transport, volume, search and the help dialog, and when they stay quiet.
 */

import type { Page } from '@playwright/test'
import { ALBUMS } from '../../support/library.ts'
import { audioState, dock, expect, expectAudioPlaying, main, openAlbum, openApp, test } from '../../support/test.ts'

const audioFlags = (page: Page) => page.evaluate(() => {
  const els = Array.from(document.querySelectorAll('audio')).filter((a) => a.currentSrc || a.src)
  const a = els.find((el) => !el.paused) ?? els[0]
  return { volume: a?.volume ?? -1, muted: a?.muted ?? false }
})

/** Leave the clicked button so Space reaches the page, not the button. */
const blur = (page: Page) => page.evaluate(() => (document.activeElement as HTMLElement | null)?.blur())

test.beforeEach(async ({ page }) => {
  await openApp(page)
  const album = ALBUMS[0]
  await openAlbum(page, album.artist, album.album)
  await main(page).getByRole('button', { name: 'Play All', exact: true }).click()
  await expectAudioPlaying(page)
  await blur(page)
})

test('Space toggles playback', async ({ page }) => {
  await page.keyboard.press('Space')
  await expect.poll(async () => (await audioState(page))?.paused).toBe(true)
  await page.keyboard.press('Space')
  await expectAudioPlaying(page, 0)
})

test('arrows seek, Shift+arrows change track and volume, M mutes', async ({ page }) => {
  const album = ALBUMS[0]
  const player = dock(page)

  await test.step('seek ±10 s', async () => {
    const before = (await audioState(page))?.currentTime ?? 0
    await page.keyboard.press('ArrowRight')
    await expect.poll(async () => (await audioState(page))?.currentTime ?? 0).toBeGreaterThan(before + 8)
    const ahead = (await audioState(page))?.currentTime ?? 0
    await page.keyboard.press('ArrowLeft')
    await expect.poll(async () => (await audioState(page))?.currentTime ?? 99).toBeLessThan(ahead - 5)
  })

  await test.step('next and previous track', async () => {
    await page.keyboard.press('Shift+ArrowRight')
    await expect(player).toContainText(album.tracks[1].title)
    await expectAudioPlaying(page, 0)
    // Back to the start first so "previous" changes track instead of restarting this one.
    await page.keyboard.press('ArrowLeft')
    await page.keyboard.press('Shift+ArrowLeft')
    await expect(player).toContainText(album.tracks[0].title)
  })

  await test.step('volume and mute', async () => {
    const { volume } = await audioFlags(page)
    await page.keyboard.press('Shift+ArrowDown')
    await expect.poll(async () => (await audioFlags(page)).volume).toBeCloseTo(Math.max(0, volume - 0.05), 2)
    await page.keyboard.press('Shift+ArrowUp')
    await expect.poll(async () => (await audioFlags(page)).volume).toBeCloseTo(volume, 2)
    await page.keyboard.press('m')
    await expect.poll(async () => (await audioFlags(page)).muted).toBe(true)
    await page.keyboard.press('m')
    await expect.poll(async () => (await audioFlags(page)).muted).toBe(false)
  })
})

test('/ focuses search, and keys typed there stay in the box', async ({ page }) => {
  await page.keyboard.press('/')
  const box = main(page).getByRole('textbox', { name: 'Search titles, artists, albums...' })
  await expect(box).toBeFocused()
  await expect(box).toHaveValue('')
  await page.keyboard.type('m m')
  await expect(box).toHaveValue('m m')
  expect((await audioState(page))?.paused).toBe(false)
  expect((await audioFlags(page)).muted).toBe(false)
})

test('? opens the shortcuts help, and keys do nothing while it is open', async ({ page }) => {
  await page.keyboard.press('?')
  const help = page.getByRole('dialog', { name: 'Keyboard shortcuts' })
  await expect(help).toBeVisible()
  for (const label of ['Play / pause', 'Seek back / forward 10 s', 'Previous / next track', 'Volume up / down', 'Mute', 'Search', 'Show shortcuts']) {
    await expect(help).toContainText(label)
  }
  await page.keyboard.press('m')
  await page.keyboard.press('Shift+ArrowRight')
  expect((await audioFlags(page)).muted).toBe(false)
  await expect(dock(page)).toContainText(ALBUMS[0].tracks[0].title)
  await help.getByRole('button', { name: 'Close' }).click()
  await expect(help).toBeHidden()
  await blur(page)
  await page.keyboard.press('Space')
  await expect.poll(async () => (await audioState(page))?.paused).toBe(true)
})
