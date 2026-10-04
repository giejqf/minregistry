import { screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, expect, it } from 'vitest';

import { RepositoryPermissionsPage } from '@/pages/permissions/repository-permissions-page';
import { ADMIN, mockApi, renderPage } from '@/test/utils';

const ci = { id: '3', kind: 'identity' as const, name: 'ci-deploy' };
const alice = { ...ADMIN, id: '4', kind: 'identity' as const, name: 'alice', is_admin: false };

function routes() {
  return {
    'GET /api/v1/repositories/5': {
      body: { id: '5', name: 'team/app', created_at: '2026-10-01T00:00:00Z', tags: [], manifests: [] },
    },
    'GET /api/v1/repositories/5/permissions': {
      body: [{ principal: ci, level: 'owner', granted_by: 'ci-deploy', granted_at: '2026-10-01T00:00:00Z' }],
    },
    'GET /api/v1/principals': {
      body: { admin_logins: [ADMIN.name], principals: [ADMIN, { ...alice, id: '3', name: 'ci-deploy' }, alice] },
    },
    'PUT /api/v1/repositories/5/permissions/4': {
      body: {
        principal: { id: '4', kind: 'identity', name: 'alice' },
        level: 'write',
        granted_at: '2026-10-03T00:00:00Z',
      },
    },
    'PUT /api/v1/repositories/5/permissions/3': {
      body: { principal: ci, level: 'read', granted_at: '2026-10-03T00:00:00Z' },
    },
  };
}

describe('RepositoryPermissionsPage', () => {
  it('lists grants and grants access to another principal', async () => {
    const { requests } = mockApi(routes());
    renderPage(<RepositoryPermissionsPage />, {
      route: '/repositories/5/permissions',
      path: '/repositories/:id/permissions',
    });

    expect(await screen.findByRole('heading', { name: 'team/app permissions' })).toBeInTheDocument();
    expect(screen.getByRole('link', { name: 'ci-deploy' })).toHaveAttribute('href', '/principals/3');

    await userEvent.click(screen.getByRole('button', { name: /grant access/i }));
    const dialog = await screen.findByRole('dialog');
    await userEvent.click(within(dialog).getByLabelText('Principal'));
    // ci-deploy already has a grant, so only the others are offered.
    expect(screen.queryByRole('option', { name: 'ci-deploy' })).not.toBeInTheDocument();
    await userEvent.click(await screen.findByRole('option', { name: 'alice' }));
    await userEvent.click(within(dialog).getByLabelText('Level'));
    await userEvent.click(await screen.findByRole('option', { name: 'write' }));
    await userEvent.click(within(dialog).getByRole('button', { name: 'Grant access' }));

    await waitFor(() => expect(requests.some((r) => r.method === 'PUT')).toBe(true));
    const put = requests.find((r) => r.method === 'PUT');
    expect(put?.url.pathname).toBe('/api/v1/repositories/5/permissions/4');
    expect(put?.body).toEqual({ level: 'write' });
  });

  it('changes a level inline', async () => {
    const { requests } = mockApi(routes());
    renderPage(<RepositoryPermissionsPage />, {
      route: '/repositories/5/permissions',
      path: '/repositories/:id/permissions',
    });
    await userEvent.click(await screen.findByRole('combobox', { name: 'Level for ci-deploy' }));
    await userEvent.click(await screen.findByRole('option', { name: 'read' }));
    const put = () => requests.find((r) => r.method === 'PUT' && r.url.pathname.endsWith('/permissions/3'));
    await waitFor(() => expect(put()?.body).toEqual({ level: 'read' }));
  });
});
