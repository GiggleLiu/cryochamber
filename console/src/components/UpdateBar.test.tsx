import { render, screen } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { UpdateBar } from './UpdateBar'
import { useAppStore, resetAppStore } from '../store/appStore'

vi.mock('../lib/swUpdate', () => ({ applyUpdate: vi.fn() }))
import { applyUpdate } from '../lib/swUpdate'

beforeEach(() => {
  resetAppStore()
  vi.mocked(applyUpdate).mockClear()
})

test('renders nothing while no update is waiting', () => {
  render(<UpdateBar />)
  expect(screen.queryByRole('status')).not.toBeInTheDocument()
})

test('offers a reload once an update is available', async () => {
  useAppStore.getState().setUpdateAvailable(true)
  render(<UpdateBar />)
  const bar = screen.getByRole('status')
  expect(bar).toHaveTextContent('Update available')
  await userEvent.click(screen.getByRole('button', { name: 'Reload' }))
  expect(applyUpdate).toHaveBeenCalledTimes(1)
})
