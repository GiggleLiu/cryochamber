import { expect, test, type Page } from '@playwright/test'
import { mockHub, signIn } from './fixtures'

const ROOT = {
  id: 'root-old',
  direction: 'outbox',
  from: 'Research Agent',
  subject: '',
  body: 'Old root equation $x^2$\n\n$$E = mc^2$$',
  timestamp: '2026-08-14T08:00:00',
  is_question: false,
}

const REPLY = {
  id: 'reply-1',
  direction: 'inbox',
  from: 'Jin-Guo Liu',
  subject: '',
  body: 'Reply equation $y_i$\n\n$$y = Ax$$',
  timestamp: '2026-08-15T10:00:00',
  is_question: false,
  thread_id: ROOT.id,
}

const MISSED_REPLY = {
  ...REPLY,
  id: 'reply-missed',
  body: 'Missed reply recovered by thread resync',
  timestamp: '2026-08-15T10:05:00',
}

const SHARED_COPY = {
  id: 'shared-copy',
  direction: 'inbox',
  from: 'Jin-Guo Liu',
  subject: '',
  body: REPLY.body,
  timestamp: '2026-08-15T14:30:00',
  is_question: false,
  shared_from: ROOT.id,
}

function recentMessages() {
  return Array.from({ length: 100 }, (_, i) => ({
    id: `recent-${String(i).padStart(3, '0')}`,
    direction: 'outbox',
    from: 'Research Agent',
    subject: '',
    body: `Recent stream message ${i + 1}`,
    timestamp: `2026-08-15T${String(11 + Math.floor(i / 60)).padStart(2, '0')}:${String(i % 60).padStart(2, '0')}:00`,
    is_question: false,
  }))
}

async function mockThreads(page: Page, sent: unknown[], messages: unknown[] = [ROOT]) {
  await mockHub(page, { chambers: [{ id: 'cham-a', name: 'qec-decoders' }] })
  await page.route('**/api/chambers/cham-a/messages*', (route) =>
    route.fulfill({ json: { messages, next: null } }),
  )
  await page.route('**/api/chambers/cham-a/threads*', (route) => {
    const url = new URL(route.request().url())
    return route.fulfill({
      json: url.searchParams.has('root')
        ? [ROOT, REPLY]
        : [{ root: ROOT, count: 1, latest: `${REPLY.timestamp} ${REPLY.id}` }],
    })
  })
  await page.route('**/api/chambers/cham-a/send', (route) => {
    sent.push(JSON.parse(route.request().postData() ?? 'null'))
    return route.fulfill({ json: { ok: true, id: `inbox/${sent.length}.md` } })
  })
  await signIn(page)
  await page.getByRole('button', { name: /qec-decoders/ }).click()
}

test('an unread old thread opens inline with math and shares only on request', async ({ page }) => {
  const sent: unknown[] = []
  await mockThreads(page, sent, [...recentMessages(), REPLY, SHARED_COPY])

  await expect(page.locator('#thread-root-old')).toHaveCount(0)
  const activity = page.getByRole('navigation', { name: 'New thread replies' })
  await expect(activity).toContainText('· 1')
  await activity.getByRole('button').click()

  const root = page.locator('#thread-root-old')
  await expect(root).toBeVisible()
  await expect(root.locator(':scope > .msg-col > .bubble .katex')).toHaveCount(2)
  const replies = root.getByRole('region', { name: 'Thread replies' })
  await expect(replies.getByText('Reply equation', { exact: false })).toBeVisible()
  await expect(replies.locator('.katex')).toHaveCount(2)
  const streamCopies = page.locator('.msg-row > .msg-col > .bubble .message-body').filter({ hasText: 'Reply equation' })
  await expect(streamCopies).toHaveCount(1)
  await expect(page.locator('#thread-shared-copy').getByText('Reply equation', { exact: false })).toBeVisible()

  expect(sent).toEqual([])
  await replies.getByRole('button', { name: 'Share to stream' }).click()
  await expect.poll(() => sent).toEqual([{ body: '', share_message_id: REPLY.id }])
  await expect(replies.getByRole('button', { name: 'Shared to stream' })).toBeDisabled()

  await root.getByRole('button', { name: 'Close thread' }).click()
  const shared = page.locator('#thread-shared-copy')
  await expect(shared.getByRole('button', { name: 'Shared from thread ↗' })).toBeVisible()
  await expect(shared.locator('.thread-toggle')).toHaveCount(0)
  await shared.getByRole('button', { name: 'Shared from thread ↗' }).click()
  await expect(root.getByRole('region', { name: 'Thread replies' })).toBeVisible()
})

test('an open thread refetches when its summary revision changes', async ({ page }) => {
  await mockHub(page, { chambers: [{ id: 'cham-a', name: 'qec-decoders' }] })
  await page.route('**/api/chambers/cham-a/messages*', (route) =>
    route.fulfill({ json: { messages: [ROOT], next: null } }),
  )
  let revised = false
  let threadFetches = 0
  await page.route('**/api/chambers/cham-a/threads*', (route) => {
    const url = new URL(route.request().url())
    if (url.searchParams.has('root')) {
      threadFetches += 1
      return route.fulfill({ json: revised ? [ROOT, REPLY, MISSED_REPLY] : [ROOT, REPLY] })
    }
    const latest = revised ? MISSED_REPLY : REPLY
    return route.fulfill({
      json: [{ root: ROOT, count: revised ? 2 : 1, latest: `${latest.timestamp} ${latest.id}` }],
    })
  })
  let releaseEvent!: () => void
  const eventReady = new Promise<void>((resolve) => { releaseEvent = resolve })
  let streams = 0
  await page.route('**/api/events', async (route) => {
    streams += 1
    if (streams > 1) return new Promise(() => {})
    await eventReady
    return route.fulfill({
      status: 200,
      headers: { 'content-type': 'text/event-stream', 'cache-control': 'no-cache' },
      body: `event: message\ndata: ${JSON.stringify({
        id: 'stream-refresh', chamber_id: 'cham-a', direction: 'outbox', from: 'Research Agent',
        subject: '', body: 'A new main-stream message', timestamp: '2026-08-15T10:06:00',
        is_question: false,
      })}\n\n`,
    })
  })

  await signIn(page)
  await page.getByRole('button', { name: /qec-decoders/ }).click()
  await page.getByRole('button', { name: /1 replies/ }).click()
  const replies = page.getByRole('region', { name: 'Thread replies' })
  await expect(replies.getByText('Reply equation', { exact: false })).toBeVisible()
  await expect(replies.getByText(MISSED_REPLY.body)).toHaveCount(0)
  const initialFetches = threadFetches
  expect(initialFetches).toBeGreaterThanOrEqual(1)

  revised = true
  releaseEvent()
  await expect(replies.getByText(MISSED_REPLY.body)).toBeVisible()
  expect(threadFetches).toBeGreaterThan(initialFetches)
  await expect(page.locator('.msg-row > .msg-col > .bubble').filter({ hasText: MISSED_REPLY.body })).toHaveCount(0)
})

test('thread attachments stage from paste and picker, preview, remove, and send without text', async ({ page }) => {
  const sent: unknown[] = []
  await mockThreads(page, sent)
  let uploads = 0
  await page.route('**/api/chambers/cham-a/uploads', (route) => {
    uploads += 1
    const name = uploads === 1 ? 'paste.png' : 'notes.txt'
    return route.fulfill({
      json: { name, markdown: `[${name}](/api/chambers/cham-a/files/${name})` },
    })
  })

  await page.getByRole('button', { name: /1 replies/ }).click()
  const replies = page.getByRole('region', { name: 'Thread replies' })
  const composer = replies.locator('.composer-dock')
  const box = composer.getByRole('textbox', { name: 'Thread reply' })
  await box.evaluate((element) => {
    const transfer = new DataTransfer()
    transfer.items.add(new File(['png'], 'paste.png', { type: 'image/png' }))
    element.dispatchEvent(new ClipboardEvent('paste', { bubbles: true, clipboardData: transfer }))
  })
  await expect(composer.getByRole('img', { name: 'paste.png' })).toBeVisible()

  await composer.locator('input[type="file"]').setInputFiles({
    name: 'notes.txt', mimeType: 'text/plain', buffer: Buffer.from('notes'),
  })
  await expect(composer.getByRole('list', { name: 'Attachments' })).toContainText('notes.txt')
  await composer.getByRole('button', { name: 'Remove notes.txt' }).click()
  await expect(composer.getByText('notes.txt')).toHaveCount(0)
  await expect(box).toHaveValue('')

  await composer.getByRole('button', { name: 'Send reply' }).click()
  await expect.poll(() => sent).toEqual([{
    body: '![paste.png](</api/chambers/cham-a/files/paste.png> "attachment:3")',
    thread_id: ROOT.id,
  }])
})

test('a failed thread attachment can be retried and sent', async ({ page }) => {
  const sent: unknown[] = []
  await mockThreads(page, sent)
  let attempts = 0
  await page.route('**/api/chambers/cham-a/uploads', (route) => {
    attempts += 1
    return attempts === 1
      ? route.fulfill({ status: 500, json: { detail: 'disk full' } })
      : route.fulfill({
          json: { name: 'retry.txt', markdown: '[retry.txt](/api/chambers/cham-a/files/retry.txt)' },
        })
  })

  await page.getByRole('button', { name: /1 replies/ }).click()
  const composer = page.getByRole('region', { name: 'Thread replies' }).locator('.composer-dock')
  await composer.locator('input[type="file"]').setInputFiles({
    name: 'retry.txt', mimeType: 'text/plain', buffer: Buffer.from('retry'),
  })
  await expect(composer.getByRole('alert')).toContainText('Could not upload retry.txt')
  await expect(composer.getByRole('button', { name: 'Send reply' })).toBeDisabled()
  await composer.getByRole('button', { name: 'Retry retry.txt' }).click()
  await expect(composer.getByRole('button', { name: 'Send reply' })).toBeEnabled()
  expect(attempts).toBe(2)
  await composer.getByRole('button', { name: 'Send reply' }).click()
  await expect.poll(() => sent).toEqual([{
    body: '[retry.txt](</api/chambers/cham-a/files/retry.txt> "attachment:5")',
    thread_id: ROOT.id,
  }])
})

test('a CSV preview escapes file content and restores focus after Escape', async ({ page }) => {
  const fileMessage = {
    ...ROOT,
    id: 'file-message',
    body: '[results.csv](</api/chambers/cham-a/files/results.csv> "attachment:35")',
  }
  await mockThreads(page, [], [fileMessage])
  await page.route('**/api/chambers/cham-a/files/results.csv', (route) => route.fulfill({
    contentType: 'text/csv',
    body: 'name,value\n<script>alert("unsafe")</script>,7',
  }))

  const preview = page.getByRole('button', { name: 'Preview results.csv' })
  await preview.click()
  const dialog = page.getByRole('dialog', { name: 'Preview results.csv' })
  await expect(dialog).toBeVisible()
  await expect(dialog.locator('.file-preview-text')).toContainText('<script>alert("unsafe")</script>')
  await expect(dialog.locator('script')).toHaveCount(0)

  await page.keyboard.press('Escape')
  await expect(dialog).toHaveCount(0)
  await expect(page.getByRole('button', { name: 'Preview results.csv' })).toBeFocused()
})

test('unsafe or unsupported PDFs fall back to download without creating a frame', async ({ page }) => {
  await page.addInitScript(() => {
    Object.defineProperty(Navigator.prototype, 'pdfViewerEnabled', {
      configurable: true,
      get: () => false,
    })
  })
  const fileMessage = {
    ...ROOT,
    id: 'pdf-files',
    body: [
      '[fake.pdf](</api/chambers/cham-a/files/fake.pdf> "attachment:31")',
      '[valid.pdf](</api/chambers/cham-a/files/valid.pdf> "attachment:9")',
    ].join('\n\n'),
  }
  await mockThreads(page, [], [fileMessage])
  await page.route('**/api/chambers/cham-a/files/fake.pdf', route => route.fulfill({
    contentType: 'application/pdf',
    body: '<html><script>alert("unsafe")</script>',
  }))
  await page.route('**/api/chambers/cham-a/files/valid.pdf', route => route.fulfill({
    contentType: 'application/octet-stream',
    body: '%PDF-1.4\n',
  }))

  await page.getByRole('button', { name: 'Preview fake.pdf' }).click()
  const fakeDialog = page.getByRole('dialog', { name: 'Preview fake.pdf' })
  await expect(fakeDialog.getByRole('alert')).toContainText('PDF preview is unavailable')
  await expect(fakeDialog.getByRole('button', { name: 'Download fake.pdf' })).toBeVisible()
  await expect(fakeDialog.locator('iframe, script')).toHaveCount(0)
  await page.keyboard.press('Escape')
  await expect(fakeDialog).toHaveCount(0)

  await page.getByRole('button', { name: 'Preview valid.pdf' }).click()
  const validDialog = page.getByRole('dialog', { name: 'Preview valid.pdf' })
  await expect(validDialog.getByRole('alert')).toContainText('PDF preview is unavailable')
  await expect(validDialog.getByRole('button', { name: 'Download valid.pdf' })).toBeVisible()
  await expect(validDialog.locator('iframe')).toHaveCount(0)
})
