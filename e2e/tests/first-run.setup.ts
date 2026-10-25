/**
 * First-run journey on a brand-new install: choose the data folder, sign in as the default admin,
 * add the fixture library and scan it. Saves the admin session for the desktop and mobile projects.
 */

import { ALBUMS, ARTISTS, TRACK_COUNT } from '../support/library.ts'
import { ADMIN_STATE, DATA_DIR, MUSIC_DIR } from '../support/paths.ts'
import { expect, goTo, openSettingsTab, test } from '../support/test.ts'

// Before setup every data route answers 503 by design.
test.use({ allowServerErrors: true })

test('first-run setup, sign-in, add library and scan', async ({ page, request }) => {
  await test.step('setup screen creates the database', async () => {
    await page.goto('/')
    await expect(page.getByRole('heading', { name: 'Set up your server' })).toBeVisible()
    await page.getByRole('textbox', { name: 'Database folder' }).fill(DATA_DIR)
    await page.getByRole('button', { name: 'Set up BoogieBox' }).click()
    await expect(page.getByRole('heading', { name: 'Select a user' })).toBeVisible()
    const status = await (await request.get('/api/system/status')).json()
    expect(status.setupRequired).toBe(false)
  })

  await test.step('default admin signs in without a PIN', async () => {
    await page.getByRole('checkbox', { name: 'Stay logged in' }).check()
    await page.getByRole('button', { name: 'Sign in as admin' }).click()
    await expect(page.getByRole('navigation').getByRole('button', { name: 'Home', exact: true })).toBeVisible()
    await expect(page.getByText('No media found yet. Add a library and run a scan.')).toBeVisible()
  })

  await test.step('add the fixture library from Settings', async () => {
    await openSettingsTab(page, 'Libraries')
    await page.getByRole('textbox', { name: /^Folder path/ }).fill(MUSIC_DIR)
    await page.getByRole('textbox', { name: 'Name (optional)' }).fill('E2E Music')
    await page.getByRole('button', { name: 'Test path' }).click()
    await expect(page.getByText(/^Path OK/)).toBeVisible()
    await page.getByRole('button', { name: 'Add library' }).click()
    await expect(page.getByText('MUSIC - 0 tracks - 1 folder')).toBeVisible()
  })

  await test.step('scan finds every fixture track', async () => {
    await page.getByRole('button', { name: 'Scan library' }).click()
    await expect(page.getByText(new RegExp(`MUSIC - ${TRACK_COUNT} tracks - 1 folder - Last scan`))).toBeVisible({ timeout: 60_000 })
    await expect(page.getByRole('group', { name: /^Library totals/ }))
      .toHaveAccessibleName(new RegExp(`${TRACK_COUNT} tracks, ${ARTISTS.length} artists, ${ALBUMS.length} albums`))
    await expect(page.getByRole('navigation').getByRole('button', { name: 'E2E Music' })).toBeVisible()
  })

  await test.step('no scan job is left running', async () => {
    await expect
      .poll(async () => (await (await page.request.get('/api/scan-jobs/active')).json()).length, { timeout: 90_000 })
      .toBe(0)
    await goTo(page, 'Home')
    await expect(page.getByText('Recent Albums')).toBeVisible()
  })

  await page.context().storageState({ path: ADMIN_STATE })
})
