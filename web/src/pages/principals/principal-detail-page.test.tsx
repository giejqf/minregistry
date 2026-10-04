import { screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, expect, it } from 'vitest';

import { PrincipalDetailPage } from '@/pages/principals/principal-detail-page';
import { ADMIN, mockApi, renderPage } from '@/test/utils';
import type { PrincipalDetail } from '@/sdk/types.gen';

const token = {
  id: '11',
  name: 'laptop',
  prefix: 'mr_abcd1',
  status: 'active' as const,
  created_at: '2026-10-01T00:00:00Z',
  created_by: 'octo-admin',
};

function detail(overrides: Partial<PrincipalDetail['principal']> = {}): PrincipalDetail {
  return { principal: { ...ADMIN, ...overrides }, tokens: [token], permissions: [] };
}

describe('PrincipalDetailPage', () => {
  it('lets the admin create their own CLI token and shows it once', async () => {
    const { requests } = mockApi({
      'GET /api/v1/principals/1': { body: detail() },
      'POST /api/v1/principals/1/tokens': {
        status: 201,
        body: { token: { ...token, id: '12', name: 'cli' }, secret: 'mr_supersecret', username: 'octo-admin' },
      },
    });
    renderPage(<PrincipalDetailPage />, { route: '/principals/1?issue=1', path: '/principals/:id' });

    // ?issue=1 opens the dialog directly ("Create my CLI token" shortcut).
    const name = await screen.findByLabelText('Name');
    expect(name).toHaveValue('cli');
    await userEvent.click(screen.getByRole('button', { name: 'Issue token' }));

    expect(await screen.findByText(/you won’t see it again/i)).toBeInTheDocument();
    expect(screen.getByText(/docker login .* -u octo-admin --password-stdin/)).toBeInTheDocument();
    expect(requests.find((r) => r.method === 'POST')?.body).toEqual({ name: 'cli', expires_at: null });
  });

  it('does not offer tokens for another admin, but allows revoking', async () => {
    const { requests } = mockApi({
      'GET /api/v1/principals/2': { body: detail({ id: '2', name: 'other-admin' }) },
      'DELETE /api/v1/principals/2/tokens/11': { status: 204 },
    });
    renderPage(<PrincipalDetailPage />, { route: '/principals/2', path: '/principals/:id' });
    expect(await screen.findByText(/only other-admin can create tokens/i)).toBeInTheDocument();
    expect(screen.queryByRole('button', { name: /issue token|create my cli token/i })).not.toBeInTheDocument();

    await userEvent.click(screen.getByRole('button', { name: 'Revoke token laptop' }));
    await userEvent.click(await screen.findByRole('button', { name: 'Revoke token' }));
    await waitFor(() => expect(requests.some((r) => r.method === 'DELETE')).toBe(true));
  });
});
