/**
 * Free music sources and scrobbling against the mock providers (support/providers.ts): Wikipedia/TheAudioDB bios,
 * ListenBrainz stats and top songs, Deezer top songs, Refresh Metadata from MusicBrainz, the main metadata switch,
 * Libre.fm sign-in and an own ListenBrainz-compatible server. Last.fm is gone from every surface.
 */

import { expect, main, openAlbum, openApp, openArtist, openSettingsTab, test } from '../../support/test.ts'
import { MOCK, PROVIDER_URL, clearProviderRequests, providerRequests } from '../../support/providers.ts'

const SWITCH = 'Use MusicBrainz, ListenBrainz, Wikipedia and TheAudioDB'

test.beforeEach(async ({ page }) => {
  // Refresh Metadata shows Cover Art Archive images straight from the browser; keep the run offline.
  await page.route('https://coverartarchive.org/**', (route) => route.abort())
  await openApp(page)
})

test('artist page: Wikipedia bio, ListenBrainz stats and ListenBrainz top songs', async ({ page }) => {
  await openArtist(page, 'Aurora Static')
  await expect(main(page).getByText(MOCK.auroraBio)).toBeVisible()
  await expect(main(page).getByRole('link', { name: 'Wikipedia', exact: true })).toHaveAttribute('href', 'https://en.wikipedia.org/wiki/Aurora_Static')
  await expect(main(page).getByRole('link', { name: 'CC BY-SA' })).toBeVisible()
  await expect(main(page).getByText('Stats from ListenBrainz')).toBeVisible()
  await expect(main(page).getByText(MOCK.auroraListens.toLocaleString('en-US'), { exact: true })).toBeVisible()
  await expect(main(page).getByText(MOCK.auroraListeners.toLocaleString('en-US'), { exact: true })).toBeVisible()

  await expect(main(page).getByText('Listens on ListenBrainz')).toBeVisible()
  await expect(main(page).getByRole('link', { name: MOCK.auroraTopSongs[0] })).toBeVisible()
  await expect(main(page).getByText('9,000 listens')).toBeVisible()
  await expect(main(page).getByText(MOCK.auroraTopSongs[5])).toHaveCount(0) // top 5 only
  await expect(main(page).getByText(/last\.fm/i)).toHaveCount(0)
})

test('artist page: TheAudioDB bio when there is no Wikipedia article; no top songs', async ({ page }) => {
  await openArtist(page, 'The Velvet Owls')
  await expect(main(page).getByText(MOCK.velvetOwlsBio)).toBeVisible()
  await expect(main(page).getByText('From TheAudioDB')).toBeVisible()
  await expect(main(page).getByText('No top songs found.')).toBeVisible()
})

test('artist page: Deezer top songs; no bio block when no source has text', async ({ page }) => {
  await openArtist(page, 'Mira Kestrel')
  await expect(main(page).getByText('Ranked by Deezer')).toBeVisible()
  await expect(main(page).getByRole('link', { name: MOCK.miraTopSongs[0] })).toBeVisible()
  await expect(main(page).getByText('Loading info…')).toHaveCount(0)
  await expect(main(page).getByText(/From (Wikipedia|TheAudioDB)/)).toHaveCount(0)
})

test('album page: Wikipedia text through the release group in the file tags', async ({ page }) => {
  await openAlbum(page, 'Aurora Static', 'Neon Tides')
  await expect(main(page).getByText(MOCK.neonTidesText)).toBeVisible()
  await expect(main(page).getByRole('link', { name: 'Wikipedia', exact: true })).toBeVisible()
})

test('Refresh Metadata lists MusicBrainz results', async ({ page }) => {
  await openAlbum(page, 'Aurora Static', 'Neon Tides')
  await main(page).getByRole('button', { name: 'Refresh metadata' }).first().click()
  const dialog = page.getByRole('dialog')
  await expect(dialog.getByText('Refresh Metadata')).toBeVisible()
  await dialog.getByRole('button', { name: 'Search', exact: true }).click()
  await expect(dialog.getByText('MusicBrainz', { exact: true })).toBeVisible()
  await expect(dialog.getByText('1 result', { exact: true })).toBeVisible()
  await expect(dialog.getByText('Neon Tides', { exact: true })).toBeVisible()
  await expect(dialog.getByRole('button', { name: 'Apply' })).toHaveCount(1)
  await dialog.getByRole('button', { name: '✕' }).click()
  await expect(dialog).toHaveCount(0)
})

test('Free music sources switch: off asks no provider, on asks again; no Last.fm anywhere in Settings', async ({ page }) => {
  const search = async () => {
    const res = await page.request.get('/api/integrations/metadata-search', { params: { artist: 'Aurora Static' } })
    expect(res.ok()).toBe(true)
    const body = await res.json() as { results: Array<{ provider: string }> }
    return body.results.map((result) => result.provider)
  }
  const box = () => main(page).getByRole('checkbox', { name: SWITCH })

  await openSettingsTab(page, 'Integrations')
  await expect(main(page).getByText('Free music sources', { exact: true })).toBeVisible()
  await expect(main(page).getByText(/last\.fm/i)).toHaveCount(0)
  await expect(box()).toBeChecked()

  try {
    // The box is controlled: it changes once the setting is saved and read back, so click and wait.
    await box().click()
    await expect(box()).not.toBeChecked()
    await expect(box()).toBeEnabled()
    await clearProviderRequests()
    expect(await search()).not.toContain('musicbrainz')
    expect((await providerRequests()).filter((r) => r.url.startsWith('/mb/'))).toEqual([])

    await openSettingsTab(page, 'Advanced')
    await expect(main(page).getByText('Turn on Free music sources under Settings › Integrations to collect tags.')).toBeVisible()
    await expect(main(page).getByRole('checkbox', { name: SWITCH })).toHaveCount(0)
    await expect(main(page).getByRole('button', { name: /Warm Last\.fm|Sync artist styles/ })).toHaveCount(0)
    await expect(main(page).getByText(/last\.fm/i)).toHaveCount(0)

    await openSettingsTab(page, 'Integrations')
    await box().click()
    await expect(box()).toBeChecked()
    await expect(box()).toBeEnabled()
    expect(await search()).toContain('musicbrainz')
  } finally {
    await page.request.put('/api/settings', { data: { radioKeylessProviders: 'true' } })
  }
})

test('Scrobbling: Libre.fm sign-in and an own server connect and disconnect', async ({ page }) => {
  // The Libre.fm sign-in page is the real site; stand in for it. The callback comes back to this server.
  await page.context().route('https://libre.fm/**', (route) => route.fulfill({ contentType: 'text/html', body: '<p>Libre.fm sign-in (e2e)</p>' }))
  await openSettingsTab(page, 'User Settings')
  const panel = page.getByRole('region', { name: 'Scrobbling' })
  await panel.scrollIntoViewIfNeeded()
  await expect(panel.getByText(/last\.fm/i)).toHaveCount(0)

  await test.step('Libre.fm signs in through its page and the callback', async () => {
    const popupOpened = page.context().waitForEvent('page')
    await panel.getByRole('button', { name: 'Connect Libre.fm' }).click()
    const popup = await popupOpened
    await popup.waitForURL(/^https:\/\/libre\.fm\/api\/auth\//)
    const callback = new URL(popup.url()).searchParams.get('cb')
    expect(callback).toMatch(/\/api\/scrobble\/librefm\/callback\?state=/)
    await popup.goto(`${callback}&token=e2e-token`)
    await expect(popup.getByText('Libre.fm connected')).toBeVisible()
    await popup.close()
    await expect(panel.getByText(`Connected to Libre.fm as ${MOCK.libreFmUser}`)).toBeVisible({ timeout: 20_000 })

    await panel.getByRole('button', { name: 'Disconnect Libre.fm' }).click()
    await page.getByRole('alertdialog').getByRole('button', { name: 'Disconnect' }).click()
    await expect(panel.getByText('Disconnected from Libre.fm')).toBeVisible()
    await expect(panel.getByRole('button', { name: 'Connect Libre.fm' })).toBeVisible()
  })

  await test.step('own server: a wrong token is refused, the right one connects', async () => {
    const address = `${PROVIDER_URL}${MOCK.ownServerPath}`
    await panel.getByLabel('Server address').fill(address)
    await panel.getByLabel('Token').fill('wrong-token')
    await panel.getByRole('button', { name: 'Connect server' }).click()
    await expect(panel.getByText(/^Error: /)).toBeVisible()

    await panel.getByLabel('Token').fill(MOCK.ownServerToken)
    await panel.getByRole('button', { name: 'Connect server' }).click()
    await expect(panel.getByText(`Connected to ${address} as ${MOCK.ownServerUser}`)).toBeVisible()

    await panel.getByRole('button', { name: 'Disconnect your server' }).click()
    await page.getByRole('alertdialog').getByRole('button', { name: 'Disconnect' }).click()
    await expect(panel.getByText('Disconnected from your server')).toBeVisible()
  })
})
