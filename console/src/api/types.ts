/** Which backend a server/credential pair talks to. Absent means 'zulip', so
 * every pre-existing server entry and stored credential keeps working. */
export type ServerKind = 'zulip' | 'hub'

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
  kind?: ServerKind
}

export interface ZulipMessage {
  id: number
  sender_full_name: string
  sender_email: string
  timestamp: number
  content: string // server-rendered HTML
  stream_id: number
  subject: string
}

export interface ZulipUser {
  user_id: number
  full_name: string
  email: string
  is_bot: boolean
}

export interface StreamSub {
  stream_id: number
  name: string
  description: string
}

export interface UnreadStreamEntry {
  stream_id: number
  topic: string
  unread_message_ids: number[]
}

export interface InitialState {
  queueId: string
  lastEventId: number
  subscriptions: StreamSub[]
  unread: UnreadStreamEntry[]
}

export interface MessageEvent {
  id: number
  type: 'message'
  message: ZulipMessage
}

export interface FlagsEvent {
  id: number
  type: 'update_message_flags'
  flag: string
  op?: 'add' | 'remove'
  operation?: 'add' | 'remove' // older Zulip servers use this field name
  messages: number[]
}

export type ZulipEvent = MessageEvent | FlagsEvent | { id: number; type: string }

export function isMessageEvent(ev: ZulipEvent): ev is MessageEvent {
  return (
    ev.type === 'message' &&
    'message' in ev &&
    typeof (ev as MessageEvent).message === 'object' &&
    (ev as MessageEvent).message !== null
  )
}

export function isReadFlagsEvent(ev: ZulipEvent): ev is FlagsEvent {
  return (
    ev.type === 'update_message_flags' &&
    (ev as FlagsEvent).flag === 'read' &&
    'messages' in ev &&
    Array.isArray((ev as FlagsEvent).messages)
  )
}
