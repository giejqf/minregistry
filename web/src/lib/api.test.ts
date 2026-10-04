import { describe, expect, it, vi } from 'vitest';

import { ApiError, errorMessage, isNotFound, isUnauthenticated, toApiError } from '@/lib/api';
import { createIdentity, getMe } from '@/sdk/sdk.gen';
import { mockApi } from '@/test/utils';

describe('generated SDK client', () => {
  it('sends the CSRF header and same-origin credentials on every request', async () => {
    const { requests } = mockApi({
      'GET /api/v1/me': { body: { principal: { id: '1' } } },
      'POST /api/v1/principals': { status: 201, body: { id: '2', name: 'ci' } },
    });
    await getMe({ throwOnError: true });
    await createIdentity({ body: { name: 'ci' }, throwOnError: true });
    expect(requests).toHaveLength(2);
    for (const req of requests) {
      expect(req.headers.get('X-Requested-With')).toBe('XMLHttpRequest');
      expect(req.credentials).toBe('same-origin');
    }
    expect(requests[1]?.body).toEqual({ name: 'ci' });
  });

  it('rejects with an ApiError carrying the status and the API error', async () => {
    mockApi({ 'GET /api/v1/me': { status: 401, body: { error: { code: 'unauthenticated', message: 'sign in' } } } });
    const error = await getMe({ throwOnError: true }).catch((e: unknown) => e);
    expect(error).toBeInstanceOf(ApiError);
    expect(isUnauthenticated(error)).toBe(true);
    expect(error).toMatchObject({
      status: 401,
      code: 'unauthenticated',
      message: 'sign in',
      error: { code: 'unauthenticated' },
    });
  });

  it('maps network failures', async () => {
    vi.stubGlobal(
      'fetch',
      vi.fn(() => Promise.reject(new TypeError('Failed to fetch'))),
    );
    const error = await getMe({ throwOnError: true }).catch((e: unknown) => e);
    expect(error).toMatchObject({ status: 0, code: 'network' });
  });
});

describe('toApiError', () => {
  const response = (status: number, statusText = '') => new Response(null, { status, statusText });

  it('uses the API error body', () => {
    const err = toApiError({ error: { code: 'conflict', message: 'taken' } }, response(409));
    expect(err).toMatchObject({ status: 409, code: 'conflict', message: 'taken' });
  });

  it('falls back to short text bodies and the status', () => {
    expect(toApiError('Bad Gateway', response(502))).toMatchObject({ status: 502, message: 'Bad Gateway (HTTP 502)' });
    expect(toApiError('<html>…</html>', response(500, 'Internal Server Error'))).toMatchObject({
      message: 'Internal Server Error (HTTP 500)',
    });
  });

  it('passes aborts through untouched', () => {
    const abort = new DOMException('aborted', 'AbortError');
    expect(toApiError(abort, undefined)).toBe(abort);
  });
});

describe('errorMessage', () => {
  it('reads every error shape', () => {
    expect(errorMessage(new ApiError(404, 'not_found', 'gone'))).toBe('gone');
    expect(isNotFound(new ApiError(404, 'not_found', 'gone'))).toBe(true);
    expect(errorMessage({ error: { code: 'x', message: 'from body' } })).toBe('from body');
    expect(errorMessage(new Error('plain'))).toBe('plain');
    expect(errorMessage(undefined)).toBe('Something went wrong.');
  });
});
