import type { HubAccount } from '../store/hubs'
import { chamberKey, splitChamberKey } from '../lib/hubKeys'
import { HubClient } from './hubClient'
import type {
  ChamberStatus, ChamberAgentUpdate, TodoItem, LifecycleAction, ActionResult,
} from './hubClient'
import type { Chamber, ChamberMessage } from './types'

export interface ConsoleClient {
  getMessages(chamberKey: string): Promise<ChamberMessage[]>
  sendMessage(chamberKey: string, body: string): Promise<{ id: string }>
  uploadFile(file: File, chamberKey: string): Promise<string>
  chamberStatus(chamberKey: string): Promise<ChamberStatus>
  setChamberAgent(chamberKey: string, agent: string): Promise<ChamberAgentUpdate>
  setChamberPlan(chamberKey: string, content: string): Promise<void>
  chamberTodos(chamberKey: string): Promise<TodoItem[]>
  lifecycle(chamberKey: string, action: LifecycleAction): Promise<ActionResult>
  fetchBlobFor(chamberKey: string, url: string): Promise<Blob>
}

export interface HubEntry {
  hub: HubAccount
  client: HubClient
}

/** N hubs behind the one client surface the views consume. Chamber keys are
 * `"{hubId}:{chamberId}"` (Task 2); the router owns the only place they are
 * split. Settings-level, hub-scoped calls (invites, host config, new chamber)
 * do not belong here — callers take `forHub(hubId)` and use the HubClient
 * directly, because those calls are *about* one hub, not about a chamber. */
export class HubRouter implements ConsoleClient {
  private readonly byId: Map<string, HubEntry>

  constructor(entries: HubEntry[]) {
    this.byId = new Map(entries.map((e) => [e.hub.id, e]))
  }

  entries(): HubEntry[] {
    return [...this.byId.values()]
  }

  forHub(hubId: string): HubClient | null {
    return this.byId.get(hubId)?.client ?? null
  }

  /** Throws for an unknown hub — which is why every method below is `async`:
   * a `Promise`-declared method that throws synchronously skips a
   * `.then(ok, err)` caller's error arm (the outbox is exactly that shape),
   * stranding the call instead of failing it. */
  private resolve(key: string): { client: HubClient; chamberId: string; hubId: string } {
    const { hubId, chamberId } = splitChamberKey(key)
    const entry = this.byId.get(hubId)
    if (!entry) throw new Error(`Unknown hub for chamber key ${key}`)
    return { client: entry.client, chamberId, hubId }
  }

  async listChambersFor(hubId: string): Promise<Chamber[]> {
    const entry = this.byId.get(hubId)
    if (!entry) throw new Error(`Unknown hub ${hubId}`)
    const list = await entry.client.listChambers()
    // The row carries its hub both ways: in the key the views pass back, and
    // as a field the store filters on and a hub chip reads.
    return list.map((c) => ({ ...c, id: chamberKey(hubId, c.id), hubId }))
  }

  toEventMessageFor(hubId: string, payload: unknown): ChamberMessage | null {
    const entry = this.byId.get(hubId)
    if (!entry) return null
    const m = entry.client.toEventMessage(payload)
    return m ? { ...m, chamberId: chamberKey(hubId, m.chamberId) } : null
  }

  async getMessages(key: string): Promise<ChamberMessage[]> {
    const { client, chamberId, hubId } = this.resolve(key)
    const msgs = await client.getMessages(chamberId)
    return msgs.map((m) => ({ ...m, chamberId: chamberKey(hubId, chamberId) }))
  }

  async sendMessage(key: string, body: string): Promise<{ id: string }> {
    const { client, chamberId } = this.resolve(key)
    return client.sendMessage(chamberId, body)
  }

  async uploadFile(file: File, key: string): Promise<string> {
    const { client, chamberId } = this.resolve(key)
    return client.uploadFile(file, chamberId)
  }

  async chamberStatus(key: string): Promise<ChamberStatus> {
    const { client, chamberId } = this.resolve(key)
    return client.chamberStatus(chamberId)
  }

  async setChamberAgent(key: string, agent: string): Promise<ChamberAgentUpdate> {
    const { client, chamberId } = this.resolve(key)
    return client.setChamberAgent(chamberId, agent)
  }

  async setChamberPlan(key: string, content: string): Promise<void> {
    const { client, chamberId } = this.resolve(key)
    return client.setChamberPlan(chamberId, content)
  }

  async chamberTodos(key: string): Promise<TodoItem[]> {
    const { client, chamberId } = this.resolve(key)
    return client.chamberTodos(chamberId)
  }

  async lifecycle(key: string, action: LifecycleAction): Promise<ActionResult> {
    const { client, chamberId } = this.resolve(key)
    return client.lifecycle(chamberId, action)
  }

  async fetchBlobFor(key: string, url: string): Promise<Blob> {
    const { client } = this.resolve(key)
    return client.fetchBlob(url)
  }
}
