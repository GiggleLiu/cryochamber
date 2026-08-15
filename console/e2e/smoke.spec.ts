import { test, expect } from '@playwright/test'
import { mockZulip, signIn } from './fixtures'

const message = {
  id: 7,
  sender_full_name: 'Research Agent',
  sender_email: 'bot@qec.example',
  timestamp: 1755100000,
  content: '<p>Scan complete: <strong>3 candidates</strong> found.</p>',
  stream_id: 1,
  subject: '',
}

test('login → projects → conversation → send', async ({ page }) => {
  let eventCalls = 0
  await page.route('**/servers.json', (r) =>
    r.fulfill({ json: [{ name: 'QEC Harness', prefix: '/zulip/qec', sendTopic: '' }] }),
  )
  await page.route('**/zulip/qec/api/v1/fetch_api_key', (r) =>
    r.fulfill({ json: { result: 'success', api_key: 'k1', email: 'me@qec.example' } }),
  )
  await page.route('**/zulip/qec/api/v1/register', (r) =>
    r.fulfill({
      json: {
        result: 'success',
        queue_id: 'q1',
        last_event_id: -1,
        subscriptions: [{ stream_id: 1, name: 'qec', description: 'QEC research project' }],
        unread_msgs: { streams: [{ stream_id: 1, topic: '', unread_message_ids: [7] }] },
      },
    }),
  )
  await page.route('**/zulip/qec/api/v1/events**', async (r) => {
    eventCalls += 1
    if (eventCalls === 1) {
      await r.fulfill({ json: { result: 'success', events: [{ id: 0, type: 'heartbeat' }] } })
    }
    // later polls: leave pending to simulate a long-poll parked at the server
  })
  await page.route('**/zulip/qec/api/v1/messages?**', (r) =>
    r.fulfill({ json: { result: 'success', messages: [message] } }),
  )
  await page.route('**/zulip/qec/api/v1/messages', (r) =>
    r.fulfill({ json: { result: 'success', id: 42 } }),
  )
  await page.route('**/zulip/qec/api/v1/mark_stream_as_read', (r) =>
    r.fulfill({ json: { result: 'success' } }),
  )

  await page.goto('/')
  await page.getByLabel(/email/i).fill('me@qec.example')
  await page.getByLabel(/password/i).fill('pw')
  await page.getByRole('button', { name: /sign in/i }).click()

  await expect(page.getByRole('button', { name: /qec/ })).toBeVisible()
  await page.getByRole('button', { name: /qec/ }).click()

  await expect(page.getByText('Scan complete:')).toBeVisible()
  await page.getByRole('textbox').fill('run the next batch')
  await page.getByRole('button', { name: /^send$/i }).click()
  await expect(page.getByRole('textbox')).toHaveValue('')
})

test.describe('phone layout contract', () => {
  test.use({ viewport: { width: 390, height: 844 } })

  test('nothing overflows sideways and every control is thumb-sized', async ({ page }) => {
    await mockZulip(page)
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
    await mockZulip(page)
    await signIn(page)
    await expect(page.getByRole('button', { name: /qec-decoders/ })).toBeVisible()
    const overflow = await page.evaluate(() => ({
      scrollWidth: document.documentElement.scrollWidth,
      clientWidth: document.documentElement.clientWidth,
    }))
    expect(overflow.scrollWidth).toBeLessThanOrEqual(overflow.clientWidth)
  })
})
