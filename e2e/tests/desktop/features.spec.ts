/**
 * Smaller feature surfaces: favourites, internet radio, themes, Subsonic switch, Activity, Home widgets.
 */

import {
  dock, expect, expectAudioPlaying, goTo, main, openAlbum, openApp, openSettingsTab, test, trackRow, uniqueName,
} from '../../support/test.ts'

test.beforeEach(async ({ page }) => openApp(page))

test('favouriting a song shows it in Favourites', async ({ page }) => {
  await openAlbum(page, 'Aurora Static', 'Night Signals')
  const row = trackRow(page, 'Antenna Bloom')
  await row.hover()
  await row.getByRole('button', { name: 'More actions' }).click()
  await page.getByRole('button', { name: 'Add to Favourites' }).click()
  await goTo(page, 'Browse Music')
  await page.getByRole('group', { name: 'Browse by' }).getByRole('button', { name: /^Favourites/ }).click()
  await expect(main(page).getByText('Antenna Bloom', { exact: true })).toBeVisible()
})

test('internet radio station: add, edit, delete', async ({ page }) => {
  const name = uniqueName('E2E Radio')
  await goTo(page, 'Browse Music')
  await page.getByRole('group', { name: 'Browse by' }).getByRole('button', { name: /^Radio/ }).click()
  await main(page).getByRole('button', { name: 'Add station' }).click()
  const form = main(page).getByRole('form', { name: 'Add station' })
  await form.getByLabel('Name').fill(name)
  await form.getByLabel('Stream URL').fill('http://127.0.0.1:9/e2e-stream.mp3')
  await form.getByRole('button', { name: 'Save' }).click()
  await expect(main(page).getByRole('button', { name: `Play ${name}` })).toBeVisible()

  await main(page).getByRole('button', { name: `Edit ${name}` }).click()
  const edit = main(page).getByRole('form', { name: 'Edit station' })
  await edit.getByLabel('Name').fill(`${name} FM`)
  await edit.getByRole('button', { name: 'Save' }).click()
  await expect(main(page).getByRole('button', { name: `Play ${name} FM` })).toBeVisible()

  await main(page).getByRole('button', { name: `Delete ${name} FM` }).click()
  await page.getByRole('button', { name: /^Delete$/ }).last().click()
  await expect(main(page).getByRole('button', { name: `Play ${name} FM` })).toHaveCount(0)
})

test('theme mode switch persists across reloads', async ({ page }) => {
  await openSettingsTab(page, 'User Settings')
  const group = page.getByRole('group', { name: 'New design theme mode' })
  await group.getByRole('button', { name: 'Use light theme' }).click()
  await expect(group.getByRole('button', { name: 'Use light theme' })).toHaveAttribute('aria-pressed', 'true')
  await page.reload()
  await openSettingsTab(page, 'User Settings')
  await expect(group.getByRole('button', { name: 'Use light theme' })).toHaveAttribute('aria-pressed', 'true')
  await group.getByRole('button', { name: 'Use dark theme' }).click()
  await expect(group.getByRole('button', { name: 'Use dark theme' })).toHaveAttribute('aria-pressed', 'true')
})

test('vintage theme renders the shell', async ({ page }) => {
  await openSettingsTab(page, 'User Settings')
  const group = page.getByRole('group', { name: 'New design theme mode' })
  await group.getByRole('button', { name: 'Use vintage theme' }).click()
  await goTo(page, 'Home')
  await expect(main(page).getByText('Recent Albums')).toBeVisible()
  await goTo(page, 'Browse Music')
  await expect(page.getByRole('group', { name: 'Browse by' })).toBeVisible()
  await openSettingsTab(page, 'User Settings')
  await group.getByRole('button', { name: 'Use dark theme' }).click()
})

test('Subsonic API switch turns /rest on and off', async ({ page }) => {
  const ping = async () => (await (await page.request.get('/rest/ping?u=admin&p=x&v=1.16.1&c=e2e&f=json')).json())['subsonic-response']
  expect((await ping()).error?.message).toMatch(/turned off/)
  await openSettingsTab(page, 'Advanced')
  const toggle = page.getByRole('switch', { name: 'Enable Subsonic API' })
  await toggle.click()
  await expect(toggle).toHaveAttribute('aria-checked', 'true')
  await expect.poll(async () => (await ping()).error?.message ?? 'ok').not.toMatch(/turned off/)
  await toggle.click()
  await expect(toggle).toHaveAttribute('aria-checked', 'false')
  await expect.poll(async () => (await ping()).error?.message ?? '').toMatch(/turned off/)
})

test('Activity shows web playback while a track plays', async ({ page }) => {
  await openAlbum(page, 'The Velvet Owls', 'Paper Moons')
  await trackRow(page, 'Folded Orbit').getByRole('button', { name: 'Play', exact: true }).click()
  await expectAudioPlaying(page)
  await goTo(page, 'Activity')
  await expect(main(page).getByText('Folded Orbit').first()).toBeVisible({ timeout: 20_000 })
  await expect(dock(page)).toContainText('Folded Orbit')
})

test('Home widgets reflect the library', async ({ page }) => {
  await goTo(page, 'Home')
  for (const album of ['Neon Tides', 'Night Signals', 'Quiet Hours', 'Paper Moons']) {
    await expect(main(page).getByRole('button', { name: `Play album ${album}`, exact: true })).toBeAttached()
  }
  await expect(main(page).getByRole('button', { name: 'Open genre Electronic' })).toBeVisible()
  await expect(main(page).getByRole('button', { name: 'New smart playlist from Electronic' })).toBeVisible()
})

test('Home genre card creates and plays a smart playlist', async ({ page }) => {
  const name = uniqueName('E2E Home Rock')
  await goTo(page, 'Home')
  await expect(main(page).getByText('Auto DJ')).toHaveCount(0)
  await main(page).getByRole('button', { name: 'New smart playlist from Rock' }).click()
  const dialog = page.getByRole('dialog', { name: 'New smart playlist' })
  await expect(dialog.getByRole('combobox', { name: 'Genre' })).toHaveValue('Rock')
  await dialog.getByLabel('Playlist name').fill(name)
  try {
    await dialog.getByRole('button', { name: 'Create and play' }).click()
    await expect(main(page).getByRole('status').filter({ hasText: `Created “${name}” and started playing.` })).toBeVisible()
    await expectAudioPlaying(page)
    await expect(main(page).getByRole('button', { name: `Play ${name}` })).toBeVisible()
  } finally {
    const list: { id: string; name: string }[] = await (await page.request.get('/api/playlists')).json()
    for (const playlist of list.filter(p => p.name === name)) await page.request.delete(`/api/playlists/${playlist.id}`)
  }
})
