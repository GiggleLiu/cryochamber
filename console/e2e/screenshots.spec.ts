import { test, expect, type Page } from '@playwright/test'
import { mockHub, signIn } from './fixtures'

/**
 * Visual harness: captures every screen of the app at a phone viewport so the
 * UI can be reviewed as images rather than as markup. Not an assertion suite —
 * `smoke.spec.ts` owns the behavioural contract — but running it does prove
 * every screen still renders.
 *
 *   npx playwright test screenshots        # → test-results/screenshots/
 *   SHOT_DIR=/tmp/shots npx playwright test screenshots
 */
const OUT = process.env.SHOT_DIR ?? 'test-results/screenshots'

test.use({ viewport: { width: 390, height: 844 }, deviceScaleFactor: 2 })

async function shot(page: Page, name: string, fullPage = false): Promise<void> {
  // Let entrance transitions settle so shots are not caught mid-animation.
  await page.waitForTimeout(400)
  await page.screenshot({ path: `${OUT}/${name}.png`, fullPage })
}

async function openConversation(page: Page): Promise<void> {
  await page.getByRole('button', { name: /qec-decoders/ }).click()
  await expect(page.getByText('Decoder sweep complete')).toBeVisible()
}

test('login', async ({ page }) => {
  await mockHub(page)
  await page.goto('/')
  await expect(page.getByRole('button', { name: /sign in/i })).toBeVisible()
  await shot(page, '01-login')

  // Keyboard focus, so the focus-visible ring is what gets captured.
  await page.keyboard.press('Tab')
  await page.keyboard.press('Tab')
  await shot(page, '02-login-focus')
})

test('login error', async ({ page }) => {
  await mockHub(page)
  await page.route('**/api/whoami', (r) => r.fulfill({ status: 401, body: '' }))
  await signIn(page)
  await expect(page.getByRole('alert')).toBeVisible()
  await shot(page, '03-login-error')
})

test('projects', async ({ page }) => {
  await mockHub(page)
  await signIn(page)
  await expect(page.getByRole('button', { name: /qec-decoders/ })).toBeVisible()
  await shot(page, '04-projects')
})

test('projects empty', async ({ page }) => {
  await mockHub(page, { chambers: [] })
  await signIn(page)
  await shot(page, '05-projects-empty')
})

test('conversation', async ({ page }) => {
  await mockHub(page)
  await signIn(page)
  await openConversation(page)
  await shot(page, '06-conversation-latest')
  await shot(page, '07-conversation-full', true)
})

test('conversation scrolled back through the thread', async ({ page }) => {
  await mockHub(page)
  await signIn(page)
  await openConversation(page)
  // The top of the thread carries the first day separator.
  await page.locator('.message-scroll').evaluate((el) => { el.scrollTop = 0 })
  await shot(page, '16-conversation-top')
  await page.getByText('Decoder sweep complete').scrollIntoViewIfNeeded()
  await shot(page, '08-conversation-markdown')
  await page.locator('.message-body table').first().scrollIntoViewIfNeeded()
  await shot(page, '09-conversation-table')
})

test('conversation loading', async ({ page }) => {
  await mockHub(page, { hangHistory: true })
  await signIn(page)
  await page.getByRole('button', { name: /qec-decoders/ }).click()
  await shot(page, '10-conversation-loading')
})

test('conversation error', async ({ page }) => {
  await mockHub(page, { failHistory: true })
  await signIn(page)
  await page.getByRole('button', { name: /qec-decoders/ }).click()
  await expect(page.getByRole('alert')).toBeVisible()
  await shot(page, '11-conversation-error')
})

test('composer with multiline and mention-like text', async ({ page }) => {
  await mockHub(page)
  await signIn(page)
  await openConversation(page)
  const box = page.getByRole('textbox', { name: /message/i })
  await box.fill('Go ahead with distance 9 tonight, and ping me when the first round lands.')
  await shot(page, '12-composer-multiline')
  await box.fill('Looping in @M')
  await expect(page.getByRole('listbox')).toHaveCount(0)
  await shot(page, '13-composer-plain-at-text')
})

test('reconnecting banner', async ({ page }) => {
  await mockHub(page, { hangRegister: true })
  await signIn(page)
  await shot(page, '14-reconnecting')
})

test('settings', async ({ page }) => {
  await mockHub(page)
  await signIn(page)
  await page.getByRole('button', { name: /settings/i }).click()
  await expect(page.getByRole('dialog')).toBeVisible()
  await shot(page, '15-settings')
})
