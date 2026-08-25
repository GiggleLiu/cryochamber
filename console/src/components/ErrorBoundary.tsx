import { Component, type ErrorInfo, type ReactNode } from 'react'
import { AlertCircle } from './Icon'

export class ErrorBoundary extends Component<
  { children: ReactNode },
  { failed: boolean }
> {
  state = { failed: false }

  static getDerivedStateFromError(): { failed: boolean } {
    return { failed: true }
  }

  componentDidCatch(error: Error, info: ErrorInfo) {
    console.error(error, info)
  }

  render() {
    if (this.state.failed) {
      return (
        <div className="empty-state" role="alert">
          <AlertCircle size={40} />
          <h2>Something went wrong</h2>
          <p>The console hit an unexpected error.</p>
          <button type="button" className="btn-primary" onClick={() => window.location.reload()}>
            Reload
          </button>
        </div>
      )
    }
    return this.props.children
  }
}
