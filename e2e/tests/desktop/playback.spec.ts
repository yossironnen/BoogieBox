/**
 * Web player: real audio playback for every fixture format, transport controls, queue and seeking.
 */

import { ALBUMS } from '../../support/library.ts'
import {
  audioState, captureMediaSession, dock, expect, expectAudioPlaying, main, mediaSession, openAlbum, openApp,
  pressMediaKey, test, trackRow,
} from '../../support/test.ts'

test.beforeEach(async ({ page }) => openApp(page))

for (const album of ALBUMS) {
  test(`plays ${album.format.toUpperCase()} audio from "${album.album}"`, async ({ page }) => {
    const track = album.tracks[0]
    await openAlbum(page, album.artist, album.album)
    await trackRow(page, track.title).getByRole('button', { name: 'Play', exact: true }).click()
    await expectAudioPlaying(page)
    await expect(dock(page)).toContainText(track.title)
    await expect(dock(page).getByRole('button', { name: `Open artist ${album.artist}` })).toBeVisible()
    await expect(dock(page).getByRole('button', { name: `Open album ${album.album}` })).toBeVisible()
  })
}

test('Play All queues the album and transport controls work', async ({ page }) => {
  const album = ALBUMS[0]
  await openAlbum(page, album.artist, album.album)
  await main(page).getByRole('button', { name: 'Play All', exact: true }).click()
  await expectAudioPlaying(page)
  const player = dock(page)
  await expect(player).toContainText(album.tracks[0].title)

  await test.step('pause and resume', async () => {
    await player.getByRole('button', { name: 'Pause' }).click()
    await expect.poll(async () => (await audioState(page))?.paused).toBe(true)
    await player.getByRole('button', { name: 'Play', exact: true }).click()
    await expectAudioPlaying(page, 0)
  })

  await test.step('next and previous', async () => {
    await player.getByRole('button', { name: 'Next track' }).click()
    await expect(player).toContainText(album.tracks[1].title)
    await expectAudioPlaying(page, 0)
    await player.getByRole('button', { name: 'Previous track' }).click()
    await expect(player).toContainText(album.tracks[0].title)
  })

  await test.step('queue panel lists the album in order and can remove a track', async () => {
    await player.getByRole('button', { name: 'Playback queue' }).click()
    const queue = page.getByRole('dialog', { name: 'Playback queue' })
    const options = queue.getByRole('listbox', { name: 'Queued tracks' }).getByRole('option')
    await expect(options).toHaveCount(album.tracks.length)
    for (const [index, track] of album.tracks.entries()) await expect(options.nth(index)).toContainText(track.title)
    const last = album.tracks[album.tracks.length - 1].title
    await queue.getByRole('button', { name: `Remove ${last} from queue` }).click()
    await expect(options).toHaveCount(album.tracks.length - 1)
    await queue.getByRole('button', { name: 'Close queue' }).click()
    await expect(queue).toBeHidden()
  })
})

test('seeking on the waveform moves playback', async ({ page }) => {
  const album = ALBUMS[2]
  await openAlbum(page, album.artist, album.album)
  await trackRow(page, album.tracks[0].title).getByRole('button', { name: 'Play', exact: true }).click()
  await expectAudioPlaying(page, 0)
  const bar = dock(page).getByRole('slider', { name: 'Waveform position' })
  const box = await bar.boundingBox()
  if (!box) throw new Error('waveform has no box')
  await page.mouse.click(box.x + box.width * 0.75, box.y + box.height / 2)
  await expect.poll(async () => (await audioState(page))?.currentTime ?? 0).toBeGreaterThan(15)
})

test('a finished track advances to the next one', async ({ page }) => {
  const album = ALBUMS[1]
  await openAlbum(page, album.artist, album.album)
  await main(page).getByRole('button', { name: 'Play All', exact: true }).click()
  await expectAudioPlaying(page, 0)
  // Jump close to the end instead of waiting the whole track out.
  await page.evaluate(() => {
    const audio = Array.from(document.querySelectorAll('audio')).find((a) => !a.paused)
    if (audio) audio.currentTime = Math.max(0, audio.duration - 1)
  })
  await expect(dock(page)).toContainText(album.tracks[1].title, { timeout: 15_000 })
  await expectAudioPlaying(page, 0)
})

test('OS media controls show the track and drive playback', async ({ page }) => {
  const album = ALBUMS[0]
  await captureMediaSession(page)
  await openApp(page)
  await openAlbum(page, album.artist, album.album)
  await main(page).getByRole('button', { name: 'Play All', exact: true }).click()
  await expectAudioPlaying(page)

  await expect.poll(async () => (await mediaSession(page)).title).toBe(album.tracks[0].title)
  const session = await mediaSession(page)
  expect(session).toMatchObject({ artist: album.artist, album: album.album, playbackState: 'playing' })
  expect(session.artwork.every((src) => /^https?:\/\/.+\/api\/albums\/.+\/art\?size=\d+$/.test(src))).toBe(true)
  expect(session.handlers).toEqual(expect.arrayContaining(['nexttrack', 'pause', 'play', 'previoustrack', 'seekto']))

  await pressMediaKey(page, 'pause')
  await expect.poll(async () => (await audioState(page))?.paused).toBe(true)
  await expect.poll(async () => (await mediaSession(page)).playbackState).toBe('paused')
  await pressMediaKey(page, 'play')
  await expectAudioPlaying(page, 0)

  await pressMediaKey(page, 'seekto', { seekTime: 20 })
  await expect.poll(async () => (await audioState(page))?.currentTime ?? 0).toBeGreaterThan(19)

  await pressMediaKey(page, 'nexttrack')
  await expect(dock(page)).toContainText(album.tracks[1].title)
  await expect.poll(async () => (await mediaSession(page)).title).toBe(album.tracks[1].title)
})

test('shuffle and repeat toggles change state', async ({ page }) => {
  const album = ALBUMS[0]
  await openAlbum(page, album.artist, album.album)
  await main(page).getByRole('button', { name: 'Play All', exact: true }).click()
  const repeat = dock(page).getByRole('button', { name: 'Repeat off' })
  await repeat.click()
  await expect(dock(page).getByRole('button', { name: /^Repeat (queue|track)$/ })).toBeVisible()
})
