import { render, screen } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { Sheet } from './Sheet'

test('renders a labelled modal dialog with its title and body', () => {
  render(
    <Sheet title="Invite to alpha" label="Invite" onClose={() => {}}>
      <p>body text</p>
    </Sheet>,
  )
  const dialog = screen.getByRole('dialog', { name: 'Invite' })
  expect(dialog).toHaveAttribute('aria-modal', 'true')
  expect(screen.getByRole('heading', { name: 'Invite to alpha' })).toBeInTheDocument()
  expect(screen.getByText('body text')).toBeInTheDocument()
})

test('the close button calls onClose', async () => {
  const onClose = vi.fn()
  render(
    <Sheet title="Chamber controls" label="Chamber controls" onClose={onClose}>
      <p>body</p>
    </Sheet>,
  )
  await userEvent.click(screen.getByRole('button', { name: 'Close' }))
  expect(onClose).toHaveBeenCalledTimes(1)
})

test('Escape closes the sheet, and focus starts inside it', async () => {
  const onClose = vi.fn()
  render(
    <Sheet title="Chamber controls" label="Chamber controls" onClose={onClose}>
      <p>body</p>
    </Sheet>,
  )
  expect(screen.getByRole('button', { name: 'Close' })).toHaveFocus()
  await userEvent.keyboard('{Escape}')
  expect(onClose).toHaveBeenCalledTimes(1)
})
