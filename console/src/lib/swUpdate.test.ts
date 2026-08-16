import { wireUpdateFlow, applyUpdate, _resetForTests } from './swUpdate'

type Listener = (e?: unknown) => void

/** A ServiceWorkerRegistration with just the surface the flow touches. */
function fakeRegistration() {
  const regListeners: Record<string, Listener> = {}
  const worker = {
    state: 'installing' as string,
    onstatechange: null as Listener | null,
    postMessage: vi.fn(),
  }
  const reg = {
    installing: null as typeof worker | null,
    waiting: null as typeof worker | null,
    addEventListener: (type: string, fn: Listener) => {
      regListeners[type] = fn
    },
  }
  return { reg, regListeners, worker }
}

const swListeners: Record<string, Listener> = {}
function stubServiceWorker(controller: object | null) {
  vi.stubGlobal('navigator', {
    serviceWorker: {
      controller,
      addEventListener: (type: string, fn: Listener) => {
        swListeners[type] = fn
      },
    },
  })
}

beforeEach(() => {
  _resetForTests()
  for (const k of Object.keys(swListeners)) delete swListeners[k]
})
afterEach(() => vi.unstubAllGlobals())

test('a worker installed while a controller exists means an update is available', () => {
  stubServiceWorker({})
  const { reg, regListeners, worker } = fakeRegistration()
  const onAvailable = vi.fn()
  wireUpdateFlow(reg as unknown as ServiceWorkerRegistration, onAvailable)
  reg.installing = worker
  regListeners.updatefound()
  worker.state = 'installed'
  worker.onstatechange?.()
  expect(onAvailable).toHaveBeenCalledTimes(1)
})

test('the very first install (no controller) is not an update', () => {
  stubServiceWorker(null)
  const { reg, regListeners, worker } = fakeRegistration()
  const onAvailable = vi.fn()
  wireUpdateFlow(reg as unknown as ServiceWorkerRegistration, onAvailable)
  reg.installing = worker
  regListeners.updatefound()
  worker.state = 'installed'
  worker.onstatechange?.()
  expect(onAvailable).not.toHaveBeenCalled()
})

test('a worker already waiting at wire time is reported immediately', () => {
  stubServiceWorker({})
  const { reg, worker } = fakeRegistration()
  worker.state = 'installed'
  reg.waiting = worker
  const onAvailable = vi.fn()
  wireUpdateFlow(reg as unknown as ServiceWorkerRegistration, onAvailable)
  expect(onAvailable).toHaveBeenCalledTimes(1)
})

test('applyUpdate posts SKIP_WAITING to the waiting worker', () => {
  stubServiceWorker({})
  const { reg, worker } = fakeRegistration()
  worker.state = 'installed'
  reg.waiting = worker
  wireUpdateFlow(reg as unknown as ServiceWorkerRegistration, () => {})
  applyUpdate()
  expect(worker.postMessage).toHaveBeenCalledWith({ type: 'SKIP_WAITING' })
})

test('applyUpdate with nothing waiting is a no-op', () => {
  stubServiceWorker({})
  expect(() => applyUpdate()).not.toThrow()
})

test('controllerchange reloads the page exactly once', () => {
  stubServiceWorker({})
  const reload = vi.fn()
  vi.stubGlobal('location', { reload })
  const { reg } = fakeRegistration()
  wireUpdateFlow(reg as unknown as ServiceWorkerRegistration, () => {})
  swListeners.controllerchange()
  swListeners.controllerchange()
  expect(reload).toHaveBeenCalledTimes(1)
})

test('the initial claim on a first install does not reload', () => {
  // No controller at wire time: this page was uncontrolled, so the
  // controllerchange that follows is the new worker's clients.claim(), not an
  // update taking over. Reloading here would make every first visit blink.
  stubServiceWorker(null)
  const reload = vi.fn()
  vi.stubGlobal('location', { reload })
  const { reg } = fakeRegistration()
  wireUpdateFlow(reg as unknown as ServiceWorkerRegistration, () => {})
  swListeners.controllerchange()
  expect(reload).not.toHaveBeenCalled()
})
