import { act, render, screen } from '@testing-library/react';
import { MemoryRouter } from 'react-router';
import { describe, expect, it } from 'vitest';

import { AuthGate } from '@/components/auth-gate';
import { useAuth } from '@/hooks/use-auth';
import { notifySessionExpired } from '@/lib/session-events';
import { Providers } from '@/providers';
import { ADMIN, mockApi, testQueryClient } from '@/test/utils';

function Whoami() {
  const { me } = useAuth();
  return <p>Signed in as {me.name}</p>;
}

function renderGate() {
  return render(
    <Providers queryClient={testQueryClient()}>
      <MemoryRouter>
        <AuthGate>
          <Whoami />
        </AuthGate>
      </MemoryRouter>
    </Providers>,
  );
}

describe('AuthGate', () => {
  it('shows the sign-in page when /me is 401', async () => {
    mockApi({ 'GET /api/v1/me': { status: 401, body: { error: { code: 'unauthenticated', message: 'no session' } } } });
    renderGate();
    expect(await screen.findByRole('button', { name: /continue with github/i })).toBeInTheDocument();
    expect(screen.getByText('MinRegistry')).toBeInTheDocument();
  });

  it('renders the app for a signed-in admin', async () => {
    mockApi({ 'GET /api/v1/me': { body: { principal: ADMIN } } });
    renderGate();
    expect(await screen.findByText('Signed in as octo-admin')).toBeInTheDocument();
  });

  it('falls back to sign-in when the session expires', async () => {
    mockApi({ 'GET /api/v1/me': { body: { principal: ADMIN } } });
    renderGate();
    await screen.findByText('Signed in as octo-admin');
    act(() => notifySessionExpired());
    expect(await screen.findByRole('button', { name: /continue with github/i })).toBeInTheDocument();
  });

  it('reports other failures with a retry', async () => {
    mockApi({
      'GET /api/v1/me': { status: 503, body: { error: { code: 'unavailable', message: 'database is locked' } } },
    });
    renderGate();
    expect(await screen.findByText('database is locked')).toBeInTheDocument();
    expect(screen.getByRole('button', { name: /try again/i })).toBeInTheDocument();
  });
});
