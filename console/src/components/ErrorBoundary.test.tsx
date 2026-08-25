import { render, screen } from '@testing-library/react'
import { ErrorBoundary } from './ErrorBoundary'

function Broken(): never {
  throw new Error('render failed')
}

test('a render crash shows the reload fallback', () => {
  const error = vi.spyOn(console, 'error').mockImplementation(() => {})
  try {
    render(
      <ErrorBoundary>
        <Broken />
      </ErrorBoundary>,
    )
    expect(screen.getByRole('heading', { name: 'Something went wrong' })).toBeInTheDocument()
    expect(screen.getByText('The console hit an unexpected error.')).toBeInTheDocument()
    expect(screen.getByRole('button', { name: 'Reload' })).toHaveClass('btn-primary')
    expect(error).toHaveBeenCalled()
  } finally {
    error.mockRestore()
  }
})
