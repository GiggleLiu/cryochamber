export class ApiError extends Error {
  constructor(
    message: string,
    readonly httpStatus: number,
    readonly code?: string,
  ) {
    super(message)
    this.name = 'ApiError'
  }
}

export function isAuthError(e: unknown): boolean {
  return e instanceof ApiError && e.httpStatus === 401
}
