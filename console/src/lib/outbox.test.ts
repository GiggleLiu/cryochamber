import { ApiError, type Credentials } from '../api/types'
import { useAppStore } from '../store/appStore'
import { sendViaOutbox } from './outbox'

const creds: Credentials = { token: 'tok', name: 'Alice', role: 'owner' }

function clientRejecting(e: unknown) {
  return { sendMessage: vi.fn().mockRejectedValue(e) } as never
}

beforeEach(() => {
  useAppStore.getState().logout()
  useAppStore.getState().setCreds(creds)
})

test("a hub-worded failure is kept on the item so the bubble can name it", async () => {
  useAppStore.setState({ client: clientRejecting(new ApiError(429, 'rate limited', true)) })
  sendViaOutbox('a', 'hello')
  await vi.waitFor(() => expect(useAppStore.getState().outboxByChamber.a[0].state).toBe('failed'))
  expect(useAppStore.getState().outboxByChamber.a[0].error).toBe('rate limited')
})

test('a transport failure the hub never worded shows no reason', async () => {
  useAppStore.setState({ client: clientRejecting(new ApiError(502, 'HTTP 502')) })
  sendViaOutbox('a', 'hello')
  await vi.waitFor(() => expect(useAppStore.getState().outboxByChamber.a[0].state).toBe('failed'))
  expect(useAppStore.getState().outboxByChamber.a[0].error).toBeNull()
})
