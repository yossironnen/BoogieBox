/**
 * Browse: artists, albums, drill-down, artwork, per-format metadata and ratings.
 */

import { ALBUMS, ARTISTS, TRACK_SECONDS } from '../../support/library.ts'
import { expect, goTo, main, openAlbum, openApp, openArtist, test, trackRow } from '../../support/test.ts'

test.beforeEach(async ({ page }) => openApp(page))

const duration = `0:${String(TRACK_SECONDS).padStart(2, '0')}`

test('Artists tab lists every artist with a count', async ({ page }) => {
  await goTo(page, 'Browse Music')
  const browseBy = page.getByRole('group', { name: 'Browse by' })
  await expect(browseBy.getByRole('button', { name: /^Artists/ })).toContainText(String(ARTISTS.length))
  for (const artist of ARTISTS) await expect(main(page).getByText(artist, { exact: true }).first()).toBeVisible()
})

test('Albums tab lists every album', async ({ page }) => {
  await goTo(page, 'Browse Music')
  await page.getByRole('group', { name: 'Browse by' }).getByRole('button', { name: /^Albums/ }).click()
  for (const album of ALBUMS) await expect(main(page).getByText(album.album, { exact: true }).first()).toBeVisible()
})

test('alphabet quick-jump enables only letters that have artists', async ({ page }) => {
  await goTo(page, 'Browse Music')
  await expect(page.getByRole('button', { name: 'Jump to A' })).toBeEnabled()
  await expect(page.getByRole('button', { name: 'B unavailable' })).toBeDisabled()
})

test('artist page shows album and track counts', async ({ page }) => {
  await openArtist(page, 'Aurora Static')
  await expect(main(page).getByText('2 albums · 5 tracks')).toBeVisible()
  await expect(main(page).getByText('Neon Tides', { exact: true })).toBeVisible()
  await expect(main(page).getByText('Night Signals', { exact: true })).toBeVisible()
})

for (const album of ALBUMS) {
  test(`${album.format.toUpperCase()} album "${album.album}" shows tracks, year and duration`, async ({ page }) => {
    await openAlbum(page, album.artist, album.album)
    await expect(main(page).getByText(new RegExp(`· ${album.year} · ${album.tracks.length} tracks`))).toBeVisible()
    for (const track of album.tracks) {
      await expect(trackRow(page, track.title)).toContainText(duration)
    }
  })
}

test('breadcrumb returns from album to artist to Browse', async ({ page }) => {
  await openAlbum(page, 'The Velvet Owls', 'Paper Moons')
  const crumbs = page.getByRole('navigation', { name: 'Breadcrumb' })
  await crumbs.getByRole('button', { name: 'The Velvet Owls' }).click()
  await expect(main(page).getByText('Paper Moons', { exact: true })).toBeVisible()
  await crumbs.getByRole('button', { name: 'Browse' }).click()
  await expect(page.getByRole('group', { name: 'Browse by' })).toBeVisible()
})

for (const album of ALBUMS) {
  test(`album art loads for "${album.album}" (${album.art} cover)`, async ({ page }) => {
    // The server reads folder.jpg/png and online providers only; covers embedded in audio files are not read yet.
    test.fixme(album.art === 'embedded', 'embedded cover art is not supported by the server')
    await openAlbum(page, album.artist, album.album)
    const cover = main(page).getByRole('img', { name: album.album }).first()
    // Artwork is cached by a post-scan job, so allow a few reloads for it to land.
    await expect(async () => {
      await page.reload()
      await openAlbum(page, album.artist, album.album)
      expect(await cover.evaluate((img: HTMLImageElement) => img.complete && img.naturalWidth > 0)).toBe(true)
    }).toPass({ timeout: 45_000, intervals: [1_000, 3_000, 5_000] })
  })
}

test('genres from every format are indexed', async ({ page }) => {
  await goTo(page, 'Search')
  const genre = main(page).getByRole('combobox').nth(1)
  for (const name of new Set(ALBUMS.map((album) => album.genre))) {
    await expect(genre.locator('option', { hasText: name })).toHaveCount(1)
  }
})

test('rating a track persists after reload', async ({ page }) => {
  await openAlbum(page, 'Mira Kestrel', 'Quiet Hours')
  const rating = main(page).getByRole('slider', { name: 'Window Seat track rating' })
  await rating.getByRole('button', { name: 'Set rating to 4 stars' }).click()
  await expect(rating).toHaveAttribute('aria-valuenow', '4')
  await page.reload()
  await openAlbum(page, 'Mira Kestrel', 'Quiet Hours')
  await expect(main(page).getByRole('slider', { name: 'Window Seat track rating' })).toHaveAttribute('aria-valuenow', '4')
})
