import { test, expect } from '@playwright/test'
import { mockHub, signIn } from './fixtures'

test('token login → projects → conversation → send', async ({ page }) => {
  const sent: unknown[] = []
  await mockHub(page, { chambers: [{ id: 'cham-a', name: 'qec-decoders' }] })
  await page.route('**/api/chambers/cham-a/send', (r) => {
    sent.push(JSON.parse(r.request().postData() ?? 'null'))
    return r.fulfill({ json: { ok: true } })
  })

  await signIn(page)

  await expect(page.getByRole('button', { name: /qec-decoders/ })).toBeVisible()
  await page.getByRole('button', { name: /qec-decoders/ }).click()

  await expect(page.getByText('Decoder sweep complete')).toBeVisible()
  await page.getByRole('textbox').fill('run the next batch')
  await page.getByRole('button', { name: /^send$/i }).click()
  await expect(page.getByRole('textbox')).toHaveValue('')
  await expect.poll(() => sent).toEqual([{ body: 'run the next batch', from: 'Jin-Guo Liu' }])
})

test.describe('phone layout contract', () => {
  test.use({ viewport: { width: 390, height: 844 } })

  test('nothing overflows sideways and every control is thumb-sized', async ({ page }) => {
    await mockHub(page)
    await signIn(page)
    await page.getByRole('button', { name: /qec-decoders/ }).click()
    await expect(page.getByText('Decoder sweep complete')).toBeVisible()

    // Wide code lines and a four-column table must scroll inside their own
    // container, never widen the page.
    const overflow = await page.evaluate(() => {
      const doc = document.documentElement
      return { scrollWidth: doc.scrollWidth, clientWidth: doc.clientWidth }
    })
    expect(overflow.scrollWidth).toBeLessThanOrEqual(overflow.clientWidth)

    for (const scroller of await page.locator('.message-body pre, .message-body table').all()) {
      const box = await scroller.boundingBox()
      expect(box!.width).toBeLessThanOrEqual(390)
    }

    // Bar actions and composer controls carry a 44px touch target.
    const controls = page.locator('.icon-btn, .send-btn, .stream-card, .mention-option')
    for (const control of await controls.all()) {
      const box = await control.boundingBox()
      expect(box!.height, await control.getAttribute('aria-label')).toBeGreaterThanOrEqual(44)
      expect(box!.width).toBeGreaterThanOrEqual(44)
    }
  })

  test('the projects list does not overflow either', async ({ page }) => {
    await mockHub(page)
    await signIn(page)
    await expect(page.getByRole('button', { name: /qec-decoders/ })).toBeVisible()
    const overflow = await page.evaluate(() => ({
      scrollWidth: document.documentElement.scrollWidth,
      clientWidth: document.documentElement.clientWidth,
    }))
    expect(overflow.scrollWidth).toBeLessThanOrEqual(overflow.clientWidth)
  })
})
