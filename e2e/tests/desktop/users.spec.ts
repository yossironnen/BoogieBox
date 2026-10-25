/**
 * Users and sign-in: PIN users, wrong PINs, the non-admin experience and user removal.
 * Sign-ins happen in fresh browser contexts so the saved admin session is never logged out.
 */

import type { Browser, Page } from '@playwright/test'
import { expect, openSettingsTab, openApp, test, uniqueName } from '../../support/test.ts'

const PIN = '4826'

async function signIn(browser: Browser, username: string, pin?: string): Promise<Page> {
  const context = await browser.newContext({ storageState: { cookies: [], origins: [] } })
  const page = await context.newPage()
  await page.goto('/')
  await page.getByRole('button', { name: `Sign in as ${username}` }).click()
  if (pin !== undefined) {
    for (const [index, digit] of [...pin].entries()) {
      // The fourth digit submits the form on its own.
      await page.getByRole('textbox', { name: `PIN digit ${index + 1}` }).fill(digit)
    }
  }
  return page
}

test('PIN user: create, sign in, restricted UI, sign out, delete', async ({ page, browser }) => {
  const username = uniqueName('listener').replace(/\s/g, '')

  await test.step('admin adds a user with a PIN', async () => {
    await openApp(page)
    await openSettingsTab(page, 'Users')
    await page.getByRole('textbox', { name: 'Username' }).fill(username)
    await page.getByRole('textbox', { name: 'PIN (optional, 4 digits)' }).fill(PIN)
    await page.getByRole('button', { name: 'Add User' }).click()
    await expect(page.getByRole('button', { name: `Delete user ${username}` })).toBeVisible()
  })

  await test.step('wrong PIN is rejected', async () => {
    const wrong = await signIn(browser, username, '0000')
    await expect(wrong.getByRole('alert')).toBeVisible()
    await expect(wrong.getByRole('navigation').getByRole('button', { name: 'Home', exact: true })).toHaveCount(0)
    await wrong.context().close()
  })

  await test.step('correct PIN signs in to a non-admin shell', async () => {
    const user = await signIn(browser, username, PIN)
    const nav = user.getByRole('navigation')
    await expect(nav.getByRole('button', { name: 'Home', exact: true })).toBeVisible()
    await expect(nav.getByRole('button', { name: 'Activity', exact: true })).toHaveCount(0)
    await nav.getByRole('button', { name: 'Settings', exact: true }).click()
    const tabs = user.getByRole('tablist', { name: 'Settings sections' })
    await expect(tabs.getByRole('tab', { name: 'User Settings' })).toBeVisible()
    await expect(tabs.getByRole('tab', { name: 'Users', exact: true })).toHaveCount(0)
    await expect(tabs.getByRole('tab', { name: 'Advanced', exact: true })).toHaveCount(0)

    await user.getByRole('button', { name: `Log out ${username}` }).click()
    await expect(user.getByRole('heading', { name: 'Select a user' })).toBeVisible()
    await user.context().close()
  })

  await test.step('admin deletes the user', async () => {
    await page.reload()
    await openSettingsTab(page, 'Users')
    await page.getByRole('button', { name: `Delete user ${username}` }).click()
    const confirm = page.getByRole('alertdialog')
    await confirm.getByRole('button').filter({ hasText: /Delete/ }).click()
    await expect(page.getByRole('button', { name: `Delete user ${username}` })).toHaveCount(0)
  })
})

test('admin sign-out and sign-in round trip', async ({ browser }) => {
  const admin = await signIn(browser, 'admin')
  await expect(admin.getByRole('navigation').getByRole('button', { name: 'Home', exact: true })).toBeVisible()
  await admin.getByRole('button', { name: 'Log out admin' }).click()
  await expect(admin.getByRole('heading', { name: 'Select a user' })).toBeVisible()
  await admin.reload()
  await expect(admin.getByRole('heading', { name: 'Select a user' })).toBeVisible()
  await admin.context().close()
})
