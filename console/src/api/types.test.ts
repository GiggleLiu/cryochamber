import { isMessageEvent, isReadFlagsEvent } from './types'
import type { AppEvent } from './types'

const messageEvent: AppEvent = {
  id: 1,
  type: 'message',
  message: {
    id: 1,
    sender_full_name: 'A',
    sender_email: 'a@b.c',
    timestamp: 1,
    content: '<p>hi</p>',
    stream_id: 1,
    subject: 'topic',
  },
}

const readFlagsEvent: AppEvent = {
  id: 2,
  type: 'update_message_flags',
  flag: 'read',
  messages: [1, 2],
}

test('well-formed message event passes isMessageEvent', () => {
  expect(isMessageEvent(messageEvent)).toBe(true)
})

test('{ id, type: "message" } without payload fails isMessageEvent', () => {
  expect(isMessageEvent({ id: 1, type: 'message' })).toBe(false)
})

test('message event with message: undefined fails isMessageEvent', () => {
  const ev: AppEvent = { id: 1, type: 'message', message: undefined }
  expect(isMessageEvent(ev)).toBe(false)
})

test('well-formed read-flags event passes isReadFlagsEvent', () => {
  expect(isReadFlagsEvent(readFlagsEvent)).toBe(true)
})

test('read-flags without messages fails isReadFlagsEvent', () => {
  expect(isReadFlagsEvent({ id: 2, type: 'update_message_flags', flag: 'read' })).toBe(false)
})

test('read-flags event with messages: undefined fails isReadFlagsEvent', () => {
  const ev: AppEvent = { id: 2, type: 'update_message_flags', flag: 'read', messages: undefined }
  expect(isReadFlagsEvent(ev)).toBe(false)
})

test('heartbeat fails both guards', () => {
  const heartbeat: AppEvent = { id: 3, type: 'heartbeat' }
  expect(isMessageEvent(heartbeat)).toBe(false)
  expect(isReadFlagsEvent(heartbeat)).toBe(false)
})
