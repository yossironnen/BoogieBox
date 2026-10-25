/**
 * Shared `test` for the UI release suite. Every test fails on uncaught page errors, app console errors and
 * HTTP 5xx responses, so a page that "looks fine" but is throwing still fails the release check.
 */

import { test as base, expect, type Locator, type Page } from '@playwright/test'

type Guard = { allowServerErrors: boolean; allowConsoleErrors: RegExp[] }

// The browser logs every non-2xx sub-resource (missing artist photos, 404 artwork retries) as a console
// error; those are expected with a tag-only fixture library and are covered by HTTP status checks instead.
const IGNORED_CONSOLE = [/Failed to load resource/]

export const test = base.extend<Guard & { guard: void }>({
  allowServerErrors: [false, { option: true }],
  allowConsoleErrors: [[], { option: true }],
  guard: [
    async ({ page, allowServerErrors, allowConsoleErrors }, use, testInfo) => {
      const problems: string[] = []
      page.on('pageerror', (error) => problems.push(`Uncaught: ${error.message}`))
      page.on('console', (msg) => {
        if (msg.type() !== 'error') return
        const text = msg.text()
        if ([...IGNORED_CONSOLE, ...allowConsoleErrors].some((re) => re.test(text))) return
        problems.push(`console.error: ${text}`)
      })
      page.on('response', (res) => {
        if (!allowServerErrors && res.status() >= 500) problems.push(`HTTP ${res.status()} ${res.request().method()} ${res.url()}`)
      })
      await use()
      if (problems.length > 0) {
        await testInfo.attach('page-problems', { body: problems.join('\n'), contentType: 'text/plain' })
        expect(problems, 'page errors, app console errors or HTTP 5xx during the test').toEqual([])
      }
    },
    { auto: true },
  ],
})

export { expect }

export function main(page: Page): Locator {
  return page.getByRole('main')
}

/** Desktop sidebar navigation. */
export async function goTo(page: Page, view: 'Home' | 'Search' | 'Browse Music' | 'Playlists' | 'Mixes' | 'Activity' | 'Settings') {
  await page.getByRole('navigation').getByRole('button', { name: view, exact: true }).click()
}

export async function openSettingsTab(page: Page, tab: string) {
  await goTo(page, 'Settings')
  await page.getByRole('tablist', { name: 'Settings sections' }).getByRole('tab', { name: tab, exact: true }).click()
}

/** Loads the app and waits for the signed-in desktop shell. */
export async function openApp(page: Page) {
  await page.goto('/')
  await expect(page.getByRole('navigation').getByRole('button', { name: 'Home', exact: true })).toBeVisible()
}

export async function openArtist(page: Page, artist: string) {
  await goTo(page, 'Browse Music')
  await page.getByRole('group', { name: 'Browse by' }).getByRole('button', { name: /^Artists/ }).click()
  await main(page).getByText(artist, { exact: true }).first().click()
  await expect(page.getByRole('navigation', { name: 'Breadcrumb' }).getByRole('button', { name: artist })).toBeVisible()
}

export async function openAlbum(page: Page, artist: string, album: string) {
  await openArtist(page, artist)
  await main(page).getByText(album, { exact: true }).first().click()
  await expect(page.getByRole('navigation', { name: 'Breadcrumb' }).getByRole('button', { name: album })).toBeVisible()
}

/** A track row in album/artist track lists: the nearest ancestor of the title that has its own Play button. */
export function trackRow(page: Page, title: string): Locator {
  return main(page).getByText(title, { exact: true }).first().locator('xpath=ancestor::*[.//button[@aria-label="Play" or @title="Play"]][1]')
}

/** The desktop playback dock. */
export function dock(page: Page): Locator {
  return page.locator('xpath=//button[@aria-label="Previous track"]/ancestor::*[.//button[@aria-label="Playback queue"]][1]')
}

export type AudioState = { src: string; currentTime: number; paused: boolean; error: number | null }

/** State of the audio element currently holding a track (the player keeps a second element for preloading). */
export async function audioState(page: Page): Promise<AudioState | null> {
  return page.evaluate(() => {
    const els = Array.from(document.querySelectorAll('audio')).filter((a) => a.currentSrc || a.src)
    const active = els.find((a) => !a.paused) ?? els[0]
    if (!active) return null
    return { src: active.currentSrc || active.src, currentTime: active.currentTime, paused: active.paused, error: active.error?.code ?? null }
  })
}

/** Waits until audio is actually advancing past `seconds`, i.e. the stream decoded and is playing. */
export async function expectAudioPlaying(page: Page, seconds = 1) {
  await expect
    .poll(async () => {
      const state = await audioState(page)
      if (state?.error) throw new Error(`Media error ${state.error} for ${state.src}`)
      return state && !state.paused ? state.currentTime : -1
    }, { timeout: 20_000, message: 'audio should start and advance' })
    .toBeGreaterThan(seconds)
}

export function uniqueName(prefix: string): string {
  return `${prefix} ${Date.now().toString(36)}`
}
