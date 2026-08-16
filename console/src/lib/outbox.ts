import { isUnauthorized } from '../api/types'
import { useAppStore, type OutboxItem } from '../store/appStore'

/** Draft storage key for a chamber's composer, namespaced per account. */
export function draftKey(account: string, chamberId: string): string {
  return `agent-console.draft.${account}.${chamberId}`
}

/** How long a `sent` bubble waits for its id to arrive before retiring itself.
 * Long enough that a healthy stream always wins the race; short enough that a
 * missed event does not leave "Sent" hanging under the thread forever. */
export const ECHO_TIMEOUT_MS = 10_000

/** Drive one send attempt for an already-queued outbox item. Success moves the
 * bubble to `sent` — it stays until the message the hub named shows up in the
 * thread, so nothing vanishes into the gap between the POST and the event; a
 * non-auth failure marks it retryable (tap to retry, never automatically: see
 * the OutboxItem docs); a 401 has already signed the app out inside the
 * client, which clears the outbox along with everything else. */
function attempt(chamberId: string, body: string, clientId: number): void {
  const store = useAppStore.getState()
  const client = store.client
  if (!client) {
    store.failOutbox(chamberId, clientId)
    return
  }
  void client.sendMessage(chamberId, body).then(
    ({ id }) => {
      useAppStore.getState().markOutboxSent(chamberId, clientId, id)
      setTimeout(() => useAppStore.getState().resolveOutbox(chamberId, clientId), ECHO_TIMEOUT_MS)
    },
    (e: unknown) => {
      if (isUnauthorized(e)) return
      useAppStore.getState().failOutbox(chamberId, clientId)
    },
  )
}

/** Queue a message and start sending it. The caller can clear its input at
 * once: from here on the pending bubble owns the text. */
export function sendViaOutbox(chamberId: string, body: string): void {
  const clientId = useAppStore.getState().enqueueOutbox(chamberId, body)
  attempt(chamberId, body, clientId)
}

/** Re-send a failed item in place, keeping its position in the thread. */
export function retryOutboxItem(item: OutboxItem): void {
  useAppStore.getState().retryOutbox(item.chamberId, item.clientId)
  attempt(item.chamberId, item.body, item.clientId)
}
