import { screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, expect, it } from 'vitest';

import { CreateIdentityDialog } from '@/pages/principals/create-identity-dialog';
import { mockApi, renderPage } from '@/test/utils';

describe('CreateIdentityDialog', () => {
  it('validates the name before calling the API', async () => {
    const { requests } = mockApi({});
    renderPage(<CreateIdentityDialog />, { route: '/principals' });
    await userEvent.click(screen.getByRole('button', { name: /create identity/i }));
    await userEvent.type(screen.getByLabelText('Name'), 'Bad Name');
    await userEvent.click(screen.getByRole('button', { name: 'Create identity' }));
    expect(await screen.findByText(/must start and end with a letter or digit/i)).toBeInTheDocument();
    expect(requests).toHaveLength(0);
  });

  it('creates the identity and opens its page', async () => {
    const { requests } = mockApi({
      'POST /api/v1/principals': {
        status: 201,
        body: {
          id: '7',
          kind: 'identity',
          name: 'ci-deploy',
          display_name: 'CI',
          enabled: true,
          is_admin: false,
          active: true,
          active_token_count: 0,
          created_at: '2026-10-03T00:00:00Z',
        },
      },
    });
    renderPage(<CreateIdentityDialog />, { route: '/principals' });
    await userEvent.click(screen.getByRole('button', { name: /create identity/i }));
    await userEvent.type(screen.getByLabelText('Name'), 'ci-deploy');
    await userEvent.type(screen.getByLabelText(/display name/i), 'CI');
    await userEvent.click(screen.getByRole('button', { name: 'Create identity' }));

    await waitFor(() => expect(screen.getByTestId('location')).toHaveTextContent('/principals/7'));
    expect(requests[0]).toMatchObject({ method: 'POST', body: { name: 'ci-deploy', display_name: 'CI' } });
    expect(requests[0]?.headers.get('X-Requested-With')).toBe('XMLHttpRequest');
  });
});
