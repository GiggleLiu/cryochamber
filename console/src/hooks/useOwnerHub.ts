import { useState } from 'react'
import { HubClient } from '../api/hubClient'
import { HubRouter } from '../api/hubRouter'
import { useAppStore } from '../store/appStore'
import type { HubAccount } from '../store/hubs'

/** Which hub an owner-only, hub-scoped control acts on right now. */
export interface OwnerHubSelection {
  /** App mode — the app holds N hubs, so "the hub" is a choice. */
  app: boolean
  /** The hubs this app owns, in the order they were added. Empty in browser
   * mode, where the one hub is the page's own and `isOwner` answers for it. */
  ownedHubs: HubAccount[]
  /** The hub the controls act on. `''` in browser mode, which is what every
   * hub-keyed store call already spells the single hub as. */
  ownerHubId: string
  /** The client those controls call, or null when there is nothing to act on. */
  ownerHub: HubClient | null
  /** Whether this session owns anything at all. */
  isOwner: boolean
  /** Point the controls at another owned hub. */
  chooseHub(hubId: string): void
}

/**
 * The owner-hub choice, shared by every sheet that drives one hub at a time
 * (settings, new chamber).
 *
 * Hub-scoped calls — host config, createChamber, invites — are *about* a hub
 * rather than about a chamber, so they take the concrete `HubClient` instead
 * of going through the router. This is the one place that narrowing happens,
 * and it answers for both modes: browser mode's client is already the hub.
 */
export function useOwnerHub(): OwnerHubSelection {
  const mode = useAppStore((s) => s.mode)
  const client = useAppStore((s) => s.client)
  const hubs = useAppStore((s) => s.hubs)
  const hubRole = useAppStore((s) => s.hubRole)
  const roleByHub = useAppStore((s) => s.roleByHub)
  const [hubChoice, setHubChoice] = useState('')

  const app = mode === 'app'
  const ownedHubs = app ? hubs.filter((h) => roleByHub[h.id] === 'owner') : []
  // The chosen hub while it is still owned, and otherwise the first — a hub can
  // be removed, or answer whoami with a role it did not have when the select
  // was last touched.
  const ownerHubId = ownedHubs.some((h) => h.id === hubChoice)
    ? hubChoice
    : (ownedHubs[0]?.id ?? '')
  return {
    app,
    ownedHubs,
    ownerHubId,
    // Browser mode owns one hub: the one that served the page. App mode owns as
    // many as its tokens do, and acts on one of them at a time.
    isOwner: app ? ownedHubs.length > 0 : hubRole === 'owner',
    ownerHub: app
      ? client instanceof HubRouter
        ? client.forHub(ownerHubId)
        : null
      : client instanceof HubClient
        ? client
        : null,
    chooseHub: setHubChoice,
  }
}
