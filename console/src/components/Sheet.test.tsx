import { fireEvent, render, screen } from '@testing-library/react'
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

test('native cancel only asks the current owner to close and preserves a guarded sheet', () => {
  const onClose = vi.fn()
  const onOuterClose = vi.fn()
  render(
    <Sheet title="Settings" label="Settings" onClose={onOuterClose}>
      <Sheet title="Edit" label="Edit" onClose={onClose}><input /></Sheet>
    </Sheet>,
  )
  const dialog = screen.getByRole('dialog', { name: 'Edit' })
  const cancel = new Event('cancel', { cancelable: true })
  fireEvent(dialog, cancel)
  expect(cancel.defaultPrevented).toBe(true)
  expect(onClose).toHaveBeenCalledTimes(1)
  expect(onOuterClose).not.toHaveBeenCalled()
  expect(dialog).toHaveAttribute('open')
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

test('a parent re-render with a new onClose does not steal focus from a field inside', async () => {
  function Parent({ tick }: { tick: number }) {
    // A fresh lambda each render, exactly like every real caller.
    return (
      <Sheet title="Invite to alpha" label="Invite" onClose={() => void tick}>
        <input aria-label="Label" />
      </Sheet>
    )
  }
  const { rerender } = render(<Parent tick={0} />)
  const input = screen.getByRole('textbox', { name: 'Label' })
  await userEvent.click(input)
  expect(input).toHaveFocus()
  rerender(<Parent tick={1} />)
  rerender(<Parent tick={2} />)
  expect(input).toHaveFocus()
})

test('Escape calls the latest onClose after a re-render', async () => {
  const first = vi.fn()
  const second = vi.fn()
  const { rerender } = render(
    <Sheet title="t" label="t" onClose={first}><p>b</p></Sheet>,
  )
  rerender(<Sheet title="t" label="t" onClose={second}><p>b</p></Sheet>)
  await userEvent.keyboard('{Escape}')
  expect(first).not.toHaveBeenCalled()
  expect(second).toHaveBeenCalledTimes(1)
})

test('focus returns to where it came from when the sheet unmounts', async () => {
  function Host({ open }: { open: boolean }) {
    return (
      <>
        <button type="button">Opener</button>
        {open && (
          <Sheet title="t" label="t" onClose={() => {}}><p>b</p></Sheet>
        )}
      </>
    )
  }
  const { rerender } = render(<Host open={false} />)
  const opener = screen.getByRole('button', { name: 'Opener' })
  opener.focus()
  rerender(<Host open={true} />)
  expect(screen.getByRole('button', { name: 'Close' })).toHaveFocus()
  rerender(<Host open={false} />)
  expect(opener).toHaveFocus()
})
