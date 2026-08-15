import { emitChamberEvent, resetChamberEvents, subscribeChamberEvents } from './chamberEvents'

beforeEach(resetChamberEvents)

test('a subscriber only hears events for its own chamber', () => {
  const mine = vi.fn()
  const theirs = vi.fn()
  subscribeChamberEvents('cham-a', mine)
  subscribeChamberEvents('cham-b', theirs)
  emitChamberEvent({ type: 'status', chamberId: 'cham-a' })
  emitChamberEvent({ type: 'log', chamberId: 'cham-b', line: 'boot' })
  expect(mine).toHaveBeenCalledExactlyOnceWith({ type: 'status', chamberId: 'cham-a' })
  expect(theirs).toHaveBeenCalledExactlyOnceWith({ type: 'log', chamberId: 'cham-b', line: 'boot' })
})

test('two sheets on one chamber both hear it, and unsubscribing only removes one', () => {
  const first = vi.fn()
  const second = vi.fn()
  const stop = subscribeChamberEvents('cham-a', first)
  subscribeChamberEvents('cham-a', second)
  emitChamberEvent({ type: 'status', chamberId: 'cham-a' })
  expect(first).toHaveBeenCalledTimes(1)
  expect(second).toHaveBeenCalledTimes(1)
  stop()
  emitChamberEvent({ type: 'status', chamberId: 'cham-a' })
  expect(first).toHaveBeenCalledTimes(1)
  expect(second).toHaveBeenCalledTimes(2)
})

test('a throwing listener does not stop the others', () => {
  const after = vi.fn()
  subscribeChamberEvents('cham-a', () => {
    throw new Error('render blew up')
  })
  subscribeChamberEvents('cham-a', after)
  expect(() => emitChamberEvent({ type: 'status', chamberId: 'cham-a' })).not.toThrow()
  expect(after).toHaveBeenCalledTimes(1)
})

test('emitting with no subscribers is a no-op', () => {
  expect(() => emitChamberEvent({ type: 'log', chamberId: 'cham-a', line: 'x' })).not.toThrow()
})
