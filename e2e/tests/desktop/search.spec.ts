/**
 * Search: full-text matches on title, artist and album, plus the genre filter.
 */

import { expect, expectAudioPlaying, goTo, main, openApp, test } from '../../support/test.ts'

test.beforeEach(async ({ page }) => {
  await openApp(page)
  await goTo(page, 'Search')
})

const box = (page: import('@playwright/test').Page) => main(page).getByRole('textbox', { name: 'Search titles, artists, albums...' })

const CASES = [
  { query: 'Kestrel', by: 'artist', count: 2, expect: ['Blue Hour Waltz', 'Window Seat'] },
  { query: 'Comet', by: 'title', count: 1, expect: ['Cardboard Comet'] },
  { query: 'Night Signals', by: 'album', count: 2, expect: ['Antenna Bloom', 'Static Lullaby'] },
]

for (const c of CASES) {
  test(`finds tracks by ${c.by}`, async ({ page }) => {
    await box(page).fill(c.query)
    await expect(main(page)).toContainText(new RegExp(String.raw`(?<!\d)${c.count} tracks?`))
    for (const title of c.expect) await expect(main(page).getByText(title, { exact: true })).toBeVisible()
  })
}

test('shows an empty state for no matches', async ({ page }) => {
  await box(page).fill('zzzz-no-such-track')
  await expect(main(page).getByText('No tracks found.')).toBeVisible()
})

// Search only runs with a query, so the genre filter is checked against one.
test('genre filter narrows results', async ({ page }) => {
  await box(page).fill('Aurora')
  await expect(main(page).getByText('Harbor Lights', { exact: true })).toBeVisible()
  await main(page).getByRole('combobox').nth(1).selectOption({ label: 'Rock (3)' })
  await expect(main(page).getByText('No tracks found.')).toBeVisible()
  await main(page).getByRole('combobox').nth(1).selectOption({ label: 'Electronic (5)' })
  await expect(main(page).getByText('Static Lullaby', { exact: true })).toBeVisible()
})

test('plays a search result', async ({ page }) => {
  await box(page).fill('Harbor Lights')
  const row = main(page).getByText('Harbor Lights', { exact: true }).locator('xpath=ancestor::*[.//button[@aria-label="Play" or @title="Play"]][1]')
  await row.getByRole('button', { name: 'Play', exact: true }).click()
  await expectAudioPlaying(page)
})
