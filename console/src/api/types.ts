/** Which backend a server/credential pair talks to. Only the chamber hub is
 * supported; the field stays so servers.json keeps documenting itself. */
export type ServerKind = 'hub'

export interface ServerConfig {
  name: string
  prefix: string
  sendTopic?: string
  kind?: ServerKind
}

export interface Credentials {
  prefix: string
  email: string
  apiKey: string
  sendTopic: string
  kind: ServerKind
}

export interface Message {
  id: number
  sender_full_name: string
  sender_email: string
  timestamp: number
  content: string // markdown source, rendered client-side
  stream_id: number
  subject: string
}

export interface User {
  user_id: number
  full_name: string
  email: string
  is_bot: boolean
}

export interface StreamSub {
  stream_id: number
  name: string
  description: string
  /** Whether the chamber's agent is awake. Absent until the hub says. */
  agentRunning?: boolean
  /** Display form of the next scheduled wake, when one is scheduled. */
  nextWake?: string | null
}

export interface UnreadStreamEntry {
  stream_id: number
  topic: string
  unread_message_ids: number[]
}

export interface InitialState {
  subscriptions: StreamSub[]
  unread: UnreadStreamEntry[]
}

export interface MessageEvent {
  id: number
  type: 'message'
  message: Message
}

export interface FlagsEvent {
  id: number
  type: 'update_message_flags'
  flag: string
  op?: 'add' | 'remove'
  messages: number[]
}

export type AppEvent = MessageEvent | FlagsEvent | { id: number; type: string }

export function isMessageEvent(ev: AppEvent): ev is MessageEvent {
  return (
    ev.type === 'message' &&
    'message' in ev &&
    typeof (ev as MessageEvent).message === 'object' &&
    (ev as MessageEvent).message !== null
  )
}

export function isReadFlagsEvent(ev: AppEvent): ev is FlagsEvent {
  return (
    ev.type === 'update_message_flags' &&
    (ev as FlagsEvent).flag === 'read' &&
    'messages' in ev &&
    Array.isArray((ev as FlagsEvent).messages)
  )
}
