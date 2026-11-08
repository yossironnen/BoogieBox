/**
 * Playlists: create, add tracks, rename, play and delete.
 */

import { IMPORTED_TITLES, writeImportFixture } from '../../support/m3u.ts'
import { dock, expect, expectAudioPlaying, goTo, main, openApp, test, uniqueName } from '../../support/test.ts'

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

test('smart playlist API: preview, create, refuse manual edits, export, analysis progress', async ({ page }) => {
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

    const summary = await (await page.request.get(`/api/playlists/${playlist.id}/summary`)).json()
    expect(summary).toMatchObject({ id: playlist.id, kind: 'smart', track_count: 3 })

    const add = await page.request.post(`/api/playlists/${playlist.id}/tracks`, { data: { track_id: snapshot.tracks[0].id } })
    expect(add.status()).toBe(409)
    expect((await add.json()).code).toBe('smart_playlist')

    const m3u = await (await page.request.get(`/api/playlists/${playlist.id}/export.m3u`)).text()
    const exported = m3u.split(/\r?\n/).filter((line) => line.startsWith('#EXTINF')).map((line) => line.split(' - ').pop())
    expect(exported).toEqual(rockTitles)

    const progress = await page.request.get(`/api/boogiemix/deep-analysis/playlists/${playlist.id}/progress`)
    expect(progress.ok()).toBe(true)
    expect(await progress.json()).toMatchObject({ total: 3, membershipRevision: 1 })

    const invalid = await page.request.post('/api/playlists/smart-preview', {
      data: { definition: { ...rules, root: { mode: 'all', children: [] } } },
    })
    expect(invalid.status()).toBe(400)
    expect((await invalid.json()).errors[0].code).toBe('empty_group')
  } finally {
    expect((await page.request.delete(`/api/playlists/${playlist.id}`)).ok()).toBe(true)
  }
})

test('smart playlist UI: build rules with a preview, rule-picked detail, save a copy', async ({ page }) => {
  const name = uniqueName('E2E Smart UI')
  const copyName = `${name} (copy)`
  const rockTitles = ['Cardboard Comet', 'Folded Orbit', 'Ink Sky']
  await goTo(page, 'Playlists')
  try {
    await test.step('build the rules', async () => {
      await main(page).getByRole('button', { name: 'New smart playlist' }).click()
      const dialog = page.getByRole('dialog', { name: 'New smart playlist' })
      await dialog.getByLabel('Playlist name').fill(name)
      await dialog.getByRole('combobox', { name: 'Field' }).selectOption('genre')
      await dialog.getByRole('combobox', { name: 'Genre' }).fill('Rock')
      await dialog.getByRole('button', { name: 'Add rule' }).click()
      await dialog.getByRole('combobox', { name: 'Field' }).nth(1).selectOption('artist')
      await dialog.getByRole('combobox', { name: 'Condition' }).nth(1).selectOption('contains')
      await dialog.getByRole('combobox', { name: 'Artist name' }).fill('velvet')
      const sample = dialog.getByRole('list', { name: 'First tracks' })
      await expect(sample.getByRole('listitem')).toHaveCount(3)
      await expect(sample).toContainText(rockTitles[0])
      await dialog.getByRole('button', { name: 'Create playlist' }).click()
      await expect(dialog).toBeHidden()
    })

    await test.step('rule-picked detail', async () => {
      await expect(main(page).getByText(name, { exact: true })).toBeVisible()
      await expect(main(page).getByText('Smart playlist', { exact: true })).toBeVisible()
      for (const title of rockTitles) await expect(main(page).getByText(title, { exact: true })).toBeVisible()
      await expect(main(page).getByRole('button', { name: 'Add tracks' })).toHaveCount(0)
      await expect(main(page).getByRole('button', { name: /from playlist$/ })).toHaveCount(0)
    })

    await test.step('save as ordinary playlist', async () => {
      await main(page).getByRole('button', { name: 'More actions' }).first().click()
      await page.getByRole('button', { name: 'Save as ordinary playlist' }).click()
      const dialog = page.getByRole('dialog', { name: 'Save as ordinary playlist' })
      await expect(dialog.getByLabel('Playlist name')).toHaveValue(copyName)
      await dialog.getByRole('button', { name: 'Save copy' }).click()
      await expect(main(page).getByText(copyName, { exact: true })).toBeVisible()
      await expect(main(page).getByRole('button', { name: 'Add tracks' })).toBeVisible()
      for (const title of rockTitles) await expect(main(page).getByText(title, { exact: true })).toBeVisible()
    })
  } finally {
    const list: { id: string; name: string }[] = await (await page.request.get('/api/playlists')).json()
    for (const playlist of list.filter(p => p.name === name || p.name === copyName)) {
      await page.request.delete(`/api/playlists/${playlist.id}`)
    }
  }
})

test('smart playlist steering pills: on, reset, queue prompt, editor line, Style rule', async ({ page }) => {
  // The fixture has 10 untagged tracks, too few for the server to offer pills, so they are turned on over the API.
  const name = uniqueName('E2E Steer')
  const shoegaze = { kind: 'style', tag: 'shoegaze', direction: 'more' }
  const popular = { kind: 'signal', signal: 'popular' }
  const rules = {
    schemaVersion: 1,
    root: { mode: 'all', children: [{ field: 'genre', operator: 'is_present' }] },
    sort: [{ field: 'title', direction: 'asc' }],
    limit: { maxTracks: 4 },
  }
  const created = await page.request.post('/api/playlists', { data: { name, kind: 'smart', definition: rules } })
  expect(created.status()).toBe(201)
  const playlist: { id: string } = await created.json()
  try {
    await test.step('the steer API stores pills and refuses bad ones', async () => {
      const put = await page.request.put(`/api/playlists/${playlist.id}/steer`, { data: { steer: [shoegaze, popular] } })
      expect(put.ok()).toBe(true)
      const answer = await put.json()
      expect(answer.steer.active).toEqual([shoegaze, popular])
      expect(answer.steer.popularSource).toBe('local')
      expect(answer.steer.fewTags).toBe(true)
      expect(answer.snapshot.tracks).toHaveLength(4)
      const view = await (await page.request.get(`/api/playlists/${playlist.id}/steer`)).json()
      expect(view.active).toEqual([shoegaze, popular])
      const refused = await page.request.put(`/api/playlists/${playlist.id}/steer`, { data: { steer: [{ kind: 'weather' }] } })
      expect(refused.status()).toBe(400)
      expect((await refused.json()).errors[0].code).toBe('bad_steer')
    })

    const pills = main(page).getByRole('group', { name: 'Steer this playlist' })
    await test.step('the detail shows the pills that are on', async () => {
      await goTo(page, 'Playlists')
      await main(page).getByText(name, { exact: true }).click()
      await expect(pills.getByRole('button', { name: 'More shoegaze', pressed: true })).toBeVisible()
      // No Last.fm counts in the fixture, so "Popular" uses the local wording.
      await expect(pills.getByRole('button', { name: 'Most played here', pressed: true })).toBeVisible()
      await expect(main(page).getByRole('status').filter({ hasText: /tracks? changed$/ })).toBeVisible()
      await expect(main(page).getByText('More pills appear once BoogieBox finds style tags for these tracks.')).toBeVisible()
    })

    await test.step('the editor lists the pills that are on', async () => {
      await main(page).getByRole('button', { name: 'Edit rules' }).first().click()
      const dialog = page.getByRole('dialog', { name: 'Edit rules' })
      await expect(dialog.getByTestId('smart-editor-pills')).toContainText('Pills on: More shoegaze, Most played here')
      await page.keyboard.press('Escape')
      await expect(dialog).toBeHidden()
    })

    await test.step('turning a pill off while the queue came from the playlist asks about the queue', async () => {
      await main(page).getByRole('button', { name: 'Play All', exact: true }).click()
      await expectAudioPlaying(page)
      await pills.getByRole('button', { name: 'More shoegaze', pressed: true }).click()
      await expect(pills.getByRole('button', { name: 'More shoegaze' })).toHaveCount(0)
      const prompt = main(page).getByRole('status').filter({ hasText: 'Your queue still has the old tracks.' })
      await expect(prompt).toBeVisible()
      await prompt.getByRole('button', { name: "Update what's next" }).click()
      await expect(prompt).toBeHidden()
      await expect(pills.getByRole('button', { name: 'Most played here', pressed: true })).toBeVisible()
    })

    await test.step('Reset turns every pill off', async () => {
      await pills.getByRole('button', { name: 'Reset' }).click()
      await expect(pills).toBeHidden()
      await expect(main(page).getByRole('status').filter({ hasText: /tracks? changed$/ })).toHaveCount(0)
      const view = await (await page.request.get(`/api/playlists/${playlist.id}/steer`)).json()
      expect(view.active).toEqual([])
      await dock(page).getByRole('button', { name: 'Pause' }).click()
    })

    await test.step('the rules editor has a Style field', async () => {
      await goTo(page, 'Playlists')
      await main(page).getByRole('button', { name: 'New smart playlist' }).click()
      const dialog = page.getByRole('dialog', { name: 'New smart playlist' })
      await dialog.getByRole('combobox', { name: 'Field' }).selectOption('style')
      const condition = dialog.getByRole('combobox', { name: 'Condition' })
      await expect(condition.getByRole('option')).toHaveText(['is', 'is not', 'is or is a kind of'])
      // No track carries a style tag: "is" picks nothing and "is not" keeps the library.
      await dialog.getByRole('combobox', { name: 'Style', exact: true }).fill('shoegaze')
      await expect(dialog.getByText('No tracks match these rules.')).toBeVisible()
      await condition.selectOption('is_not')
      await expect(dialog.getByRole('list', { name: 'First tracks' }).getByRole('listitem').first()).toBeVisible()
      await dialog.getByRole('button', { name: 'Cancel' }).click()
      await expect(dialog).toBeHidden()
    })
  } finally {
    await page.request.delete(`/api/playlists/${playlist.id}`)
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
