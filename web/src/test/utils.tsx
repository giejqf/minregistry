import { QueryClient } from '@tanstack/react-query';
import { render } from '@testing-library/react';
import type { ReactElement, ReactNode } from 'react';
import { MemoryRouter, Route, Routes, useLocation } from 'react-router';
import { vi } from 'vitest';

import { AuthContext, type AuthState } from '@/lib/auth-context';
import { Providers } from '@/providers';
import type { PrincipalSummary } from '@/sdk/types.gen';

export const ADMIN: PrincipalSummary = {
  id: '1',
  kind: 'github',
  name: 'octo-admin',
  display_name: 'Octo Admin',
  enabled: true,
  is_admin: true,
  active: true,
  active_token_count: 0,
  created_at: '2026-01-01T00:00:00Z',
};

export interface MockResponse {
  status?: number;
  body?: unknown;
}

export interface RecordedRequest {
  method: string;
  url: URL;
  headers: Headers;
  credentials: RequestCredentials;
  body: unknown;
}

type Handler = (req: RecordedRequest) => MockResponse | undefined;

/**
 * Replaces `fetch` with a router over `METHOD /path` keys (query strings are
 * ignored in keys; inspect `requests` for them). Unmatched requests get a 404.
 */
export function mockApi(routes: Record<string, MockResponse | Handler>) {
  const requests: RecordedRequest[] = [];
  const fetchMock = vi.fn(async (input: RequestInfo | URL, init?: RequestInit) => {
    const request = input instanceof Request ? input : new Request(input, init);
    const text = request.body ? await request.text() : '';
    const recorded: RecordedRequest = {
      method: request.method,
      url: new URL(request.url),
      headers: request.headers,
      credentials: request.credentials,
      body: text ? (JSON.parse(text) as unknown) : undefined,
    };
    requests.push(recorded);
    const route = routes[`${recorded.method} ${recorded.url.pathname}`];
    const result = typeof route === 'function' ? route(recorded) : route;
    const { status = 200, body } = result ?? {
      status: 404,
      body: { error: { code: 'not_found', message: `no mock for ${recorded.method} ${recorded.url.pathname}` } },
    };
    if (status === 204) return new Response(null, { status });
    return new Response(JSON.stringify(body ?? {}), { status, headers: { 'Content-Type': 'application/json' } });
  });
  vi.stubGlobal('fetch', fetchMock);
  return { requests, fetchMock };
}

export function testQueryClient(): QueryClient {
  return new QueryClient({
    defaultOptions: { queries: { retry: false, staleTime: Infinity }, mutations: { retry: false } },
  });
}

/** Renders the current router location, for asserting navigation. */
function LocationProbe() {
  const location = useLocation();
  return (
    <div data-testid="location" hidden>
      {location.pathname + location.search}
    </div>
  );
}

interface RenderOptions {
  route?: string;
  /** Route pattern the element is mounted at, e.g. `/principals/:id`. */
  path?: string;
  auth?: Partial<AuthState> | null;
  queryClient?: QueryClient;
}

export function renderPage(
  element: ReactElement,
  { route = '/', path = '*', auth = {}, queryClient = testQueryClient() }: RenderOptions = {},
) {
  const withAuth = (children: ReactNode) =>
    auth === null ? (
      children
    ) : (
      <AuthContext.Provider value={{ me: ADMIN, signOut: () => Promise.resolve(), ...auth }}>
        {children}
      </AuthContext.Provider>
    );
  return {
    queryClient,
    ...render(
      <Providers queryClient={queryClient}>
        {withAuth(
          <MemoryRouter initialEntries={[route]}>
            <Routes>
              <Route path={path} element={element} />
            </Routes>
            <LocationProbe />
          </MemoryRouter>,
        )}
      </Providers>,
    ),
  };
}
