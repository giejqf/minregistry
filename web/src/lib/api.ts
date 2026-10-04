import { client } from '@/sdk/client.gen';
import type { ErrorDetail } from '@/sdk/types.gen';

/**
 * Every failed SDK call rejects with an ApiError. It is structurally an
 * `ErrorResponse` (`{ error: { code, message } }`, the API's error body), so
 * the generated error types stay accurate, and it also carries the HTTP status.
 */
export class ApiError extends Error {
  readonly status: number;
  readonly error: ErrorDetail;

  constructor(status: number, code: string, message: string) {
    super(message);
    this.name = 'ApiError';
    this.status = status;
    this.error = { code, message };
  }

  get code(): string {
    return this.error.code;
  }
}

function isErrorBody(value: unknown): value is { error: ErrorDetail } {
  if (typeof value !== 'object' || value === null || !('error' in value)) return false;
  const detail = value.error;
  return (
    typeof detail === 'object' &&
    detail !== null &&
    typeof (detail as { code?: unknown }).code === 'string' &&
    typeof (detail as { message?: unknown }).message === 'string'
  );
}

function isAbort(error: unknown): boolean {
  return error instanceof DOMException && error.name === 'AbortError';
}

/** Normalizes whatever the fetch client rejected with into an ApiError. */
export function toApiError(error: unknown, response: Response | undefined): unknown {
  if (error instanceof ApiError || isAbort(error)) return error;
  const status = response?.status ?? 0;
  if (isErrorBody(error)) return new ApiError(status, error.error.code, error.error.message);
  if (!response) {
    return new ApiError(0, 'network', 'Could not reach the MinRegistry server. Check your connection and try again.');
  }
  const text = typeof error === 'string' ? error.trim() : '';
  const reason = text && text.length <= 200 && !text.startsWith('<') ? text : response.statusText || 'Request failed';
  return new ApiError(status, 'http', `${reason} (HTTP ${status})`);
}

let installed = false;

/** Installs the error interceptor on the generated client (idempotent). */
export function setupApiClient(): void {
  if (installed) return;
  installed = true;
  client.interceptors.error.use((error, response) => toApiError(error, response));
}

export function isUnauthenticated(error: unknown): boolean {
  return error instanceof ApiError && error.status === 401;
}

export function isNotFound(error: unknown): boolean {
  return error instanceof ApiError && error.status === 404;
}

/** A human-readable message for any error a query or mutation can produce. */
export function errorMessage(error: unknown): string {
  if (error instanceof ApiError) return error.message;
  if (isErrorBody(error)) return error.error.message;
  if (error instanceof Error && error.message) return error.message;
  if (typeof error === 'string' && error) return error;
  return 'Something went wrong.';
}
