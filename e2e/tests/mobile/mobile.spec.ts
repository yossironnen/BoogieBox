/**
 * Mobile shell (phone viewport): tabs, browse drill-down, search and playback with the mini player.
 */

import { captureMediaSession, expect, expectAudioPlaying, mediaSession, pressMediaKey, test } from '../../support/test.ts'
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
