import { HubClient } from '../api/hubClient'
import { useAppStore } from '../store/appStore'

/** Shown when a token no longer opens the hub — the invite-link case, which is
 * the one users actually hit (the owner revoked it, or it expired). */
export const INVALID_INVITE_REASON = 'This invite link is no longer valid.'

/** Shown when the `#invite=` fragment never looked like a token at all: a link
 * truncated by a chat client, a copy that lost half its characters. Distinct
 * from the revoked case, because the fix is different — ask for the link again. */
export const MALFORMED_INVITE_REASON = 'This invite link is not valid.'

/**
 * Exchange a hub access token (owner or invite) for a signed-in session: ask
 * the hub who the bearer is, record the role, then store the credentials.
 * Rejects when the token is rejected — callers decide how to say so, since the
 * invite-link path and the paste-a-token path word it differently.
 */
export async function signInWithHubToken(
  prefix: string,
  token: string,
  sendTopic = '',
): Promise<void> {
  const probe = new HubClient({ kind: 'hub', prefix, email: '', apiKey: token, sendTopic })
  const who = await probe.whoami()
  const store = useAppStore.getState()
  store.setHubRole(who.role)
  // `name` is what the hub labels this token's messages with; it is also the
  // identity the composer sends as, so it doubles as the account email field.
  store.setCreds({ kind: 'hub', prefix, email: who.name ?? 'human', apiKey: token, sendTopic })
}
