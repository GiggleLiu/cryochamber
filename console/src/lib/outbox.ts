import { isUnauthorized } from '../api/types'
import { useAppStore, AUTH_LOGOUT_REASON, type OutboxItem } from '../store/appStore'

/** Draft storage key for a project's composer, namespaced per account — two
 * tokens each number their own chambers from 1. */
export function draftKey(account: string, streamId: number): string {
  return `agent-console.draft.${account}.${streamId}`
}

/** How long a `sent` bubble waits for its echo before retiring itself. Long
 * enough that a healthy stream always wins the race; short enough that a missed
 * event does not leave "Sent" hanging under the thread forever. */
export const ECHO_TIMEOUT_MS = 10_000

/** Drive one send attempt for an already-queued outbox item. Success moves the
 * bubble to `sent` — it stays until the message itself shows up in the thread,
 * so nothing vanishes into a gap between the POST and the event; a non-auth
 * failure marks it retryable (tap to retry, never automatically: see the
 * OutboxItem docs); a 401 takes the app's single logout path, which clears the
 * outbox along with everything else. */
function attempt(streamId: number, streamName: string, content: string, localId: number): void {
  const client = useAppStore.getState().client
  if (!client) {
    useAppStore.getState().failOutbox(streamId, localId)
    return
  }
  void client.sendMessage(streamName, content).then(
    () => {
      useAppStore.getState().markOutboxSent(streamId, localId)
      setTimeout(() => useAppStore.getState().resolveOutbox(streamId, localId), ECHO_TIMEOUT_MS)
    },
    (e: unknown) => {
      if (isUnauthorized(e)) {
        useAppStore.getState().logout(AUTH_LOGOUT_REASON)
        return
      }
      useAppStore.getState().failOutbox(streamId, localId)
    },
  )
}

/** Queue a message and start sending it. The caller can clear its input at once:
 * from here on the pending bubble owns the text. */
export function sendViaOutbox(streamId: number, streamName: string, content: string): void {
  const localId = useAppStore.getState().enqueueOutbox(streamId, content)
  attempt(streamId, streamName, content, localId)
}

/** Re-send a failed item in place, keeping its position in the thread. */
export function retryOutboxItem(item: OutboxItem, streamName: string): void {
  useAppStore.getState().retryOutbox(item.streamId, item.localId)
  attempt(item.streamId, streamName, item.content, item.localId)
}
