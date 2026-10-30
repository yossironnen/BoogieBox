/**
 * Playlists: create, add tracks, rename, play and delete.
 */

import { IMPORTED_TITLES, writeImportFixture } from '../../support/m3u.ts'
import { expect, expectAudioPlaying, goTo, main, openApp, test, uniqueName } from '../../support/test.ts'

test.beforeEach(async ({ page }) => openApp(page))

test('playlist lifecycle', async ({ page }) => {
  const name = uniqueName('E2E Playlist')
  const renamed = `${name} (renamed)`
  await goTo(page, 'Playlists')

  await test.step('create', async () => {
    await main(page).getByRole('button', { name: 'New playlist' }).first().click()
    const dialog = page.getByRole('dialog', { name: 'New Playlist' })
    await expect(dialog.getByRole('button', { name: 'Create' })).toBeDisabled()
    await dialog.getByRole('textbox', { name: 'Playlist name' }).fill(name)
    await dialog.getByRole('button', { name: 'Create' }).click()
    await expect(main(page).getByText(name, { exact: true })).toBeVisible()
    await expect(main(page).getByText('0 tracks')).toBeVisible()
  })

  await test.step('add tracks from search', async () => {
    await main(page).getByRole('button', { name: 'Add tracks' }).click()
    const search = main(page).getByRole('textbox', { name: 'Search for tracks to add…' })
    for (const title of ['Ink Sky', 'Window Seat']) {
      await search.fill(title)
      const result = main(page).getByText(title, { exact: true }).last().locator('xpath=ancestor::*[.//button][1]')
      await result.getByRole('button', { name: 'Add to playlist' }).click()
      await expect(main(page).getByRole('button', { name: `Remove ${title} from playlist` })).toBeVisible()
    }
    await expect(main(page).getByText('2 tracks')).toBeVisible()
    await main(page).getByRole('button', { name: 'Close add tracks' }).click()
  })

  await test.step('options popup does not duplicate the detail view', async () => {
    await main(page).getByRole('button', { name: 'More actions' }).click()
    const options = page.getByRole('dialog', { name })
    await expect(options).toBeVisible()
    // App re-renders on its 5 s scan poll; a key clash used to stack a new copy of the detail view each time.
    await page.waitForTimeout(6_000)
    await expect(page.locator('[data-ui-region="playlist-detail"]')).toHaveCount(1)
  })

  await test.step('rename', async () => {
    await page.getByRole('dialog', { name }).getByRole('button', { name: 'Rename' }).click()
    const dialog = page.getByRole('dialog', { name: 'Rename Playlist' })
    await dialog.getByRole('textbox', { name: 'Playlist name' }).fill(renamed)
    await dialog.getByRole('button', { name: 'Save' }).click()
    await expect(dialog).toBeHidden()
    await page.keyboard.press('Escape')
    await expect(main(page).getByText(renamed, { exact: true })).toBeVisible()
  })

  await test.step('play all', async () => {
    await main(page).getByRole('button', { name: 'Play All', exact: true }).click()
    await expectAudioPlaying(page)
  })

  await test.step('shows on the playlists grid and Home', async () => {
    await main(page).getByRole('button', { name: 'Back to Playlists' }).click()
    await expect(main(page).getByText(renamed, { exact: true })).toBeVisible()
    await goTo(page, 'Home')
    await expect(main(page).getByRole('button', { name: `Open playlist ${renamed}` })).toBeVisible()
  })

  await test.step('delete', async () => {
    await goTo(page, 'Playlists')
    await main(page).getByText(renamed, { exact: true }).click()
    await main(page).getByRole('button', { name: 'More actions' }).click()
    await page.getByRole('dialog', { name: renamed }).getByRole('button', { name: 'Delete playlist' }).click()
    const confirm = page.getByRole('dialog', { name: 'Delete Playlist' })
    await confirm.getByRole('button', { name: 'Delete', exact: true }).click()
    await expect(main(page).getByText(renamed, { exact: true })).toHaveCount(0)
  })
})

test('import an M3U: preview counts, not-found list, then the playlist in file order', async ({ page }) => {
  const name = uniqueName('E2E Import')
  const file = writeImportFixture(`${name}.m3u8`)
  await goTo(page, 'Playlists')

  const chooser = page.waitForEvent('filechooser')
  await main(page).getByRole('button', { name: 'Import playlist' }).click()
  await (await chooser).setFiles(file)

  const dialog = page.getByRole('dialog', { name: 'Import playlist' })
  await expect(dialog.getByText('· 7 lines')).toBeVisible()
  await expect(dialog.getByRole('textbox', { name: 'Playlist name' })).toHaveValue(name)
  const tile = (key: string) => dialog.locator(`[data-import-tile="${key}"]`)
  await expect(tile('matched')).toContainText('4')
  await expect(tile('ambiguous')).toContainText('0')
  await expect(tile('unmatched')).toContainText('1')
  await expect(tile('remote')).toContainText('1')
  await expect(tile('duplicate')).toContainText('1')
  await expect(dialog.locator('[data-import-missing]')).toContainText('Missing Song.mp3')
  await expect(dialog.getByText("Web links aren't imported. BoogieBox plays only music in your libraries.")).toBeVisible()
  await expect(dialog.getByRole('status')).toHaveText("4 tracks will be added, in the file's order")

  const download = page.waitForEvent('download')
  await dialog.getByRole('button', { name: 'Download the list' }).click()
  expect((await download).suggestedFilename()).toBe(`${name} - not found.txt`)

  await dialog.getByRole('button', { name: 'Create playlist' }).click()
  await expect(dialog).toBeHidden()
  await expect(page.getByRole('status').filter({ hasText: 'Playlist imported' })).toContainText('4 tracks added. 3 lines were left out.')
  await expect(main(page).getByText(name, { exact: true })).toBeVisible()

  const playlists: Array<{ id: string; name: string }> = await (await page.request.get('/api/playlists')).json()
  const created = playlists.find((p) => p.name === name)!
  const tracks: Array<{ title: string }> = await (await page.request.get(`/api/playlists/${created.id}/tracks`)).json()
  expect(tracks.map((t) => t.title)).toEqual(IMPORTED_TITLES)
  expect((await page.request.delete(`/api/playlists/${created.id}`)).ok()).toBe(true)
})

test('smart playlist API: preview, create, refuse manual edits, export', async ({ page }) => {
  const rules = {
    schemaVersion: 1,
    root: { mode: 'all', children: [
      { field: 'genre', operator: 'is', value: 'Rock' },
      { field: 'artist', operator: 'contains', value: 'velvet' },
    ] },
    sort: [{ field: 'title', direction: 'asc' }],
  }
  const rockTitles = ['Cardboard Comet', 'Folded Orbit', 'Ink Sky']

  const preview = await (await page.request.post('/api/playlists/smart-preview', { data: { definition: rules } })).json()
  expect(preview.trackCount).toBe(3)
  expect(preview.tracks.map((t: { title: string }) => t.title)).toEqual(rockTitles)

  const name = uniqueName('E2E Smart')
  const created = await page.request.post('/api/playlists', { data: { name, kind: 'smart', definition: rules } })
  expect(created.status()).toBe(201)
  const playlist: { id: string; kind: string; track_count: number } = await created.json()
  expect(playlist).toMatchObject({ kind: 'smart', track_count: 3 })

  try {
    const snapshot = await (await page.request.get(`/api/playlists/${playlist.id}/snapshot`)).json()
    expect(snapshot.tracks.map((t: { title: string }) => t.title)).toEqual(rockTitles)
    expect(snapshot.refreshError).toBeNull()

    const add = await page.request.post(`/api/playlists/${playlist.id}/tracks`, { data: { track_id: snapshot.tracks[0].id } })
    expect(add.status()).toBe(409)
    expect((await add.json()).code).toBe('smart_playlist')

    const m3u = await (await page.request.get(`/api/playlists/${playlist.id}/export.m3u`)).text()
    const exported = m3u.split(/\r?\n/).filter((line) => line.startsWith('#EXTINF')).map((line) => line.split(' - ').pop())
    expect(exported).toEqual(rockTitles)

    const invalid = await page.request.post('/api/playlists/smart-preview', {
      data: { definition: { ...rules, root: { mode: 'all', children: [] } } },
    })
    expect(invalid.status()).toBe(400)
    expect((await invalid.json()).errors[0].code).toBe('empty_group')
  } finally {
    expect((await page.request.delete(`/api/playlists/${playlist.id}`)).ok()).toBe(true)
  }
})

test('duplicate playlist names are rejected', async ({ page }) => {
  const name = uniqueName('E2E Dup')
  await goTo(page, 'Playlists')
  for (let attempt = 0; attempt < 2; attempt += 1) {
    if (attempt === 1) await main(page).getByRole('button', { name: 'Back to Playlists' }).click()
    await main(page).getByRole('button', { name: 'New playlist' }).first().click()
    const dialog = page.getByRole('dialog', { name: 'New Playlist' })
    await dialog.getByRole('textbox', { name: 'Playlist name' }).fill(name)
    await dialog.getByRole('button', { name: 'Create' }).click()
  }
  await expect(page.getByRole('dialog', { name: 'New Playlist' }).getByText('A playlist with this name already exists')).toBeVisible()
})
