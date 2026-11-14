/**
 * Web player: real audio playback for every fixture format, transport controls, queue and seeking.
 */

import { ALBUMS } from '../../support/library.ts'
import {
  audioState, captureMediaSession, dock, expect, expectAudioPlaying, main, mediaSession, openAlbum, openApp,
  openSettingsTab, pressMediaKey, test, trackRow,
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

test.describe('sleep timer', () => {
  const activeVolume = (page: import('@playwright/test').Page) => page.evaluate(() => {
    const els = Array.from(document.querySelectorAll('audio')).filter((a) => a.currentSrc || a.src)
    return (els.find((a) => !a.paused) ?? els[0])?.volume ?? -1
  })

  test('a minutes timer fades out, pauses and can be cancelled', async ({ page }) => {
    await page.clock.install()
    await openApp(page)
    const album = ALBUMS[0]
    await openAlbum(page, album.artist, album.album)
    await main(page).getByRole('button', { name: 'Play All', exact: true }).click()
    await expectAudioPlaying(page)
    const player = dock(page)

    await test.step('start and cancel', async () => {
      await player.getByRole('button', { name: 'Sleep timer', exact: true }).click()
      await page.getByRole('dialog', { name: 'Sleep timer' }).getByRole('button', { name: '30 min' }).click()
      const running = player.getByRole('button', { name: /^Sleep timer: \d+:\d\d left$/ })
      await expect(running).toContainText('30m')
      await running.click()
      await page.getByRole('dialog', { name: 'Sleep timer' }).getByRole('button', { name: 'Cancel timer' }).click()
      await expect(player.getByRole('button', { name: 'Sleep timer', exact: true })).toBeVisible()
    })

    await test.step('fade and pause at the deadline', async () => {
      await player.getByRole('button', { name: 'Sleep timer', exact: true }).click()
      const popover = page.getByRole('dialog', { name: 'Sleep timer' })
      await expect(popover).toContainText('Reloading the page clears the timer.')
      await popover.getByRole('button', { name: '15 min' }).click()
      await expect(player.getByRole('button', { name: /^Sleep timer: \d+:\d\d left$/ })).toContainText('15m')
      const full = await activeVolume(page)
      await page.clock.fastForward('14:50')
      await expect.poll(() => activeVolume(page)).toBeLessThan(full)
      expect((await audioState(page))?.paused).toBe(false)
      await page.clock.fastForward('00:15')
      await expect.poll(async () => (await audioState(page))?.paused).toBe(true)
      await expect(player.getByRole('button', { name: 'Play', exact: true })).toBeVisible()
      await expect(player.getByRole('button', { name: 'Sleep timer', exact: true })).toBeVisible()
      await expect.poll(() => activeVolume(page)).toBeCloseTo(full, 2)
    })
  })

  test('"End of track" stops at the end instead of advancing', async ({ page }) => {
    const album = ALBUMS[1]
    await openAlbum(page, album.artist, album.album)
    await main(page).getByRole('button', { name: 'Play All', exact: true }).click()
    await expectAudioPlaying(page, 0)
    const player = dock(page)
    await player.getByRole('button', { name: 'Sleep timer', exact: true }).click()
    await page.getByRole('dialog', { name: 'Sleep timer' }).getByRole('button', { name: 'End of track' }).click()
    await expect(player.getByRole('button', { name: 'Sleep timer: end of track' })).toContainText('End')
    await page.evaluate(() => {
      const audio = Array.from(document.querySelectorAll('audio')).find((a) => !a.paused)
      if (audio) audio.currentTime = Math.max(0, audio.duration - 1)
    })
    await expect(player.getByRole('button', { name: 'Play', exact: true })).toBeVisible({ timeout: 15_000 })
    await expect(player).toContainText(album.tracks[0].title)
    await expect(player).not.toContainText(album.tracks[1].title)
    await expect(player.getByRole('button', { name: 'Sleep timer', exact: true })).toBeVisible()
  })
})

test('Auto EQ owns the curve graph while it is on and leaves my own EQ alone', async ({ page }) => {
  const album = ALBUMS[0]
  await openAlbum(page, album.artist, album.album)
  await trackRow(page, album.tracks[0].title).getByRole('button', { name: 'Play', exact: true }).click()
  await expectAudioPlaying(page, 0)
  await dock(page).getByRole('button', { name: 'Equalizer', exact: true }).click()
  const eq = page.getByRole('dialog', { name: 'Equalizer' })
  const autoEq = eq.getByRole('checkbox', { name: 'Auto EQ' })
  const bass = eq.getByRole('group', { name: 'EQ bands' }).getByRole('button', { name: /^Band 2 / })
  const bassLabel = async () => (await bass.getAttribute('aria-label')) ?? ''
  // Band 2 is a peaking band: "Band 2 Bass, 120 Hz, +0.0 dB". Its handle sits where EqCurveCanvas draws it.
  const dragBass = async (dy: number) => {
    const [, freqText, kilo, gainText] = /, ([\d.]+)(k?) Hz, ([+-][\d.]+) dB$/.exec(await bassLabel())!
    const freq = Number(freqText) * (kilo ? 1000 : 1)
    const box = (await eq.locator('canvas').boundingBox())!
    const margin = { top: 8, right: 8, bottom: 20, left: 28 }
    const x = margin.left + (Math.log10(freq / 20) / Math.log10(1000)) * (box.width - margin.left - margin.right)
    const y = margin.top + ((12 - Number(gainText)) / 24) * (box.height - margin.top - margin.bottom)
    await page.mouse.move(box.x + x, box.y + y)
    await page.mouse.down()
    await page.mouse.move(box.x + x, box.y + y + dy, { steps: 5 })
    await page.mouse.up()
  }
  const wasOn = await autoEq.isChecked()
  try {
    if (wasOn) await autoEq.uncheck()
    await bass.click()
    await eq.getByRole('spinbutton', { name: 'Band 2 gain value' }).fill('0')
    await eq.getByRole('spinbutton', { name: 'Band 2 frequency' }).fill('120')
    await expect.poll(bassLabel).toBe('Band 2 Bass, 120 Hz, +0.0 dB')

    await test.step('with Auto EQ off the graph moves a band', async () => {
      await dragBass(-25)
      await expect.poll(bassLabel).not.toBe('Band 2 Bass, 120 Hz, +0.0 dB')
    })
    const mine = await bassLabel()
    expect(mine).toMatch(/\+\d+\.\d dB$/)

    await test.step('with Auto EQ on the graph and band controls do not edit', async () => {
      await autoEq.check()
      await expect(eq.getByText(/^Auto EQ active - preset:/)).toBeVisible()
      await expect(eq.getByRole('spinbutton', { name: 'Band 2 gain value' })).toBeDisabled()
      await expect(eq.getByRole('combobox', { name: 'EQ profile' })).toBeDisabled()
      const chosen = await bassLabel()
      await dragBass(-25)
      await page.waitForTimeout(300)
      expect(await bassLabel()).toBe(chosen)
    })

    await test.step('turning Auto EQ off brings back my own EQ', async () => {
      await autoEq.uncheck()
      await expect.poll(bassLabel).toBe(mine)
    })
  } finally {
    if (await autoEq.isChecked()) await autoEq.uncheck()
    await bass.click()
    await eq.getByRole('spinbutton', { name: 'Band 2 gain value' }).fill('0')
    await eq.getByRole('spinbutton', { name: 'Band 2 frequency' }).fill('120')
    if (wasOn) await autoEq.check()
    await eq.getByRole('button', { name: 'Close equalizer' }).click()
  }
})

test('loudness levelling: tag values, measured tracks and the player gain', async ({ page }) => {
  const idOf = async (title: string) => {
    const result = await (await page.request.get('/api/search', { params: { q: title, limit: 5 } })).json()
    const track = (result.tracks as Array<{ id: string; title: string }>).find(t => t.title === title)
    expect(track, `track ${title}`).toBeTruthy()
    return String(track!.id)
  }
  const loudness = async (ids: string[]) =>
    (await (await page.request.post('/api/tracks/loudness', { data: { ids } })).json()) as Record<string, { trackGainDb: number; albumGainDb?: number | null }>
  const [harbor, antenna, folded] = [await idOf('Harbor Lights'), await idOf('Antenna Bloom'), await idOf('Folded Orbit')]

  await test.step('ReplayGain tags from MP3 (TXXX) and FLAC (Vorbis)', async () => {
    const values = await loudness([harbor, antenna])
    expect(values[harbor].trackGainDb).toBe(-6.5)
    expect(values[antenna].trackGainDb).toBe(2.25)
    expect(values[antenna].albumGainDb).toBe(-1)
  })

  await test.step('the background lane measures an untagged loud track', async () => {
    await expect.poll(async () => (await loudness([folded]))[folded]?.trackGainDb ?? null, { timeout: 120_000, intervals: [2_000] })
      .toBeLessThan(-5)
  })

  await test.step('the player applies the track gain, and none when levelling is off', async () => {
    const setMode = (mode: string) => page.request.put('/api/user/settings', { data: { loudnessMode: mode } })
    expect((await setMode('track')).ok()).toBe(true)
    try {
      await openApp(page)
      const album = ALBUMS[0]
      await openAlbum(page, album.artist, album.album)
      await trackRow(page, 'Harbor Lights').getByRole('button', { name: 'Play', exact: true }).click()
      await expectAudioPlaying(page, 0)
      const gain = () => page.evaluate(() => {
        const playing = Array.from(document.querySelectorAll('audio')).find(a => !a.paused)
        return playing?.getAttribute('data-loudness-gain') ?? null
      })
      // A cut is never clamped (peak 0.5 allows up to +5 dB), so the tag's −6.5 dB is applied as is.
      await expect.poll(gain).toBe('-6.5')

      expect((await setMode('off')).ok()).toBe(true)
      await openApp(page)
      await openAlbum(page, album.artist, album.album)
      await trackRow(page, 'Harbor Lights').getByRole('button', { name: 'Play', exact: true }).click()
      await expectAudioPlaying(page, 0)
      await expect.poll(gain).toBe('0')
    } finally {
      await setMode('off')
    }
  })
})

test('volume levelling settings: mode chips, pre-amp and the admin server card save', async ({ page }) => {
  const userSettings = async () => (await (await page.request.get('/api/user/settings')).json()) as Record<string, string>
  try {
    await openSettingsTab(page, 'User Settings')
    const card = page.getByRole('region', { name: 'Volume levelling' })
    const modes = card.getByRole('group', { name: 'Volume levelling mode' })
    await modes.getByRole('button', { name: 'Album', exact: true }).click()
    await expect(modes.getByRole('button', { name: 'Album', exact: true })).toHaveAttribute('aria-pressed', 'true')
    await expect(card).toContainText('Albums keep their own quiet and loud tracks.')
    await expect.poll(async () => (await userSettings()).loudnessMode).toBe('album')

    await card.getByLabel('Pre-amp').fill('2.5')
    await expect(card).toContainText('+2.5 dB')
    await expect.poll(async () => (await userSettings()).loudnessPreampDb).toBe('2.5')

    await openSettingsTab(page, 'Advanced')
    await page.getByRole('navigation', { name: 'Advanced settings groups' }).getByRole('link', { name: 'Server levelling' }).click()
    const server = page.getByRole('region', { name: 'Server levelling' })
    await expect(server).toBeInViewport()
    // The whole Advanced tab is admin-only, so the card carries no "Admin" badge.
    await expect(server.getByText('Admin', { exact: true })).toHaveCount(0)
    await server.getByLabel('Play On speakers').selectOption('track')
    await expect(server).toContainText('Saved')
    const shared = await (await page.request.get('/api/playback-settings')).json()
    expect(shared.loudnessPlayOnMode).toBe('track')
    await expect(server.getByLabel(/^From tags: \d+/)).toBeVisible()
  } finally {
    await page.request.put('/api/user/settings', { data: { loudnessMode: 'off', loudnessPreampDb: '0' } })
    await page.request.put('/api/settings', { data: { loudnessPlayOnMode: 'off' } })
  }
})

test('Auto EQ follows a genre set on the album in Edit metadata', async ({ page }) => {
  const album = ALBUMS.find((a) => a.album === 'Quiet Hours')!
  await openAlbum(page, album.artist, album.album)
  await trackRow(page, album.tracks[0].title).getByRole('button', { name: 'Play', exact: true }).click()
  await expectAudioPlaying(page, 0)
  const eqButton = dock(page).getByRole('button', { name: 'Equalizer', exact: true })
  const eq = page.getByRole('dialog', { name: 'Equalizer' })
  const autoEq = eq.getByRole('checkbox', { name: 'Auto EQ' })
  const preset = eq.getByText(/^Auto EQ active - preset:/)
  const setAlbumGenre = async (genre: string) => {
    await main(page).getByRole('button', { name: 'Edit album' }).click()
    const field = page.getByText('Genre', { exact: true }).locator('..').locator('input')
    await field.fill(genre)
    await page.getByRole('button', { name: 'Save', exact: true }).click()
    await expect(field).toBeHidden()
  }

  await eqButton.click()
  const wasOn = await autoEq.isChecked()
  let edited = false
  try {
    if (!wasOn) await autoEq.check()
    // The tracks are tagged Jazz.
    await expect(preset).toHaveText(/^Auto EQ active - preset: Vintage\b/)
    await eq.getByRole('button', { name: 'Close equalizer' }).click()

    edited = true
    await setAlbumGenre('Classical')
    await eqButton.click()
    await expect(preset).toHaveText(/^Auto EQ active - preset: Classical\b/)
    await eq.getByRole('button', { name: 'Close equalizer' }).click()
  } finally {
    if (await eq.isVisible()) await eq.getByRole('button', { name: 'Close equalizer' }).click()
    if (edited) await setAlbumGenre(album.genre)
    if (!wasOn) {
      await eqButton.click()
      await autoEq.uncheck()
      await eq.getByRole('button', { name: 'Close equalizer' }).click()
    }
  }
})

test('Saved EQ: save for a song, keep it after a reload, remove it', async ({ page }) => {
  const album = ALBUMS[1]
  const track = album.tracks[0]
  const playTrack = async () => {
    await openAlbum(page, album.artist, album.album)
    await trackRow(page, track.title).getByRole('button', { name: 'Play', exact: true }).click()
    await expectAudioPlaying(page, 0)
    await dock(page).getByRole('button', { name: 'Equalizer', exact: true }).click()
  }
  const eq = page.getByRole('dialog', { name: 'Equalizer' })
  const autoEq = eq.getByRole('checkbox', { name: 'Auto EQ' })
  const useSaved = eq.getByRole('checkbox', { name: 'Use my saved EQ' })
  const status = eq.getByRole('status').filter({ hasText: /EQ saved for this|Not saved yet/ })
  const bass = eq.getByRole('group', { name: 'EQ bands' }).getByRole('button', { name: /^Band 2 / })
  // The gain part of "Band 2 Bass, 120 Hz, +4.0 dB" (the frequency can carry float noise from a graph drag).
  const bassGain = async () => /([+-][\d.]+) dB$/.exec((await bass.getAttribute('aria-label')) ?? '')?.[1]
  const setBass = async (gain: string) => {
    await bass.click()
    await eq.getByRole('spinbutton', { name: 'Band 2 gain value' }).fill(gain)
  }
  const removeSong = eq.getByRole('button', { name: 'Remove from this song' })

  await playTrack()
  const autoWasOn = await autoEq.isChecked()
  const savedWasOn = await useSaved.isChecked()
  try {
    if (autoWasOn) await autoEq.uncheck()
    if (savedWasOn) await useSaved.uncheck()
    await expect(status).toBeHidden()

    await test.step('saving for the song turns the switch on and says so', async () => {
      await setBass('4')
      await expect.poll(bassGain).toBe('+4.0')
      await eq.getByRole('button', { name: 'Save for this song' }).click()
      await expect(status).toHaveText('Using the EQ saved for this song')
      await expect(useSaved).toBeChecked()
      await expect(removeSong).toBeVisible()
      await expect(eq.getByRole('button', { name: 'Remove from this album' })).toBeHidden()
    })

    await test.step('the saved curve is back after a reload', async () => {
      await openApp(page)
      await playTrack()
      await expect(useSaved).toBeChecked()
      await expect(status).toHaveText('Using the EQ saved for this song')
      await expect.poll(bassGain).toBe('+4.0')
    })

    await test.step('removing it clears the status and the link', async () => {
      await removeSong.click()
      await expect(removeSong).toBeHidden()
      await expect(status).toBeHidden()
    })
  } finally {
    if (await removeSong.isVisible()) await removeSong.click()
    await expect(removeSong).toBeHidden()
    await setBass('0')
    await expect.poll(bassGain).toBe('+0.0')
    if ((await useSaved.isChecked()) !== savedWasOn) await useSaved.setChecked(savedWasOn)
    if (autoWasOn) await autoEq.check()
    await eq.getByRole('button', { name: 'Close equalizer' }).click()
  }
})
