import { screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, expect, it } from 'vitest';

import { AuditPage } from '@/pages/audit-page';
import { mockApi, renderPage, type RecordedRequest } from '@/test/utils';

const event = {
  id: '120',
  ts: '2026-10-03T10:00:00Z',
  principal_id: '3',
  principal_name: 'ci-deploy',
  action: 'manifest.push',
  repository: 'team/app',
  reference: 'v1',
  digest: 'sha256:0123456789abcdef0123456789abcdef',
  client_ip: '10.0.0.7',
  user_agent: 'docker/27.0',
  outcome: 'denied',
  detail: { reason: 'insufficient permission', level: 'read' },
};

function auditReply(req: RecordedRequest) {
  return { body: { items: [event], next_cursor: req.url.searchParams.get('cursor') ? null : '120' } };
}

const common = {
  'GET /api/v1/audit/actions': { body: { actions: ['manifest.push', 'manifest.pull'] } },
  'GET /api/v1/principals': { body: { admin_logins: [], principals: [] } },
  'GET /api/v1/repositories': { body: { items: [], total: 0 } },
  'GET /api/v1/audit': auditReply,
};

describe('AuditPage', () => {
  it('sends the URL filters to the API and expands rows', async () => {
    const { requests } = mockApi(common);
    renderPage(<AuditPage />, { route: '/audit?repository=team%2Fapp&outcome=denied' });

    expect(await screen.findByText('ci-deploy')).toBeInTheDocument();
    const auditCall = requests.find((r) => r.url.pathname === '/api/v1/audit');
    expect(Object.fromEntries(auditCall?.url.searchParams ?? [])).toEqual({
      repository: 'team/app',
      outcome: 'denied',
      limit: '50',
    });
    expect(within(screen.getByRole('table')).getByText('denied')).toBeInTheDocument();
    expect(screen.getByLabelText('Repository')).toHaveValue('team/app');

    await userEvent.click(screen.getByRole('button', { name: 'Show details' }));
    expect(screen.getByText('10.0.0.7')).toBeInTheDocument();
    expect(screen.getByText('docker/27.0')).toBeInTheDocument();
    expect(screen.getByText(/"reason": "insufficient permission"/)).toBeInTheDocument();
  });

  it('pages with Older/Newer and keeps filters in the URL', async () => {
    mockApi(common);
    renderPage(<AuditPage />, { route: '/audit?outcome=denied' });
    await screen.findByText('ci-deploy');

    const newer = screen.getByRole('button', { name: /newer/i });
    expect(newer).toBeDisabled();
    await userEvent.click(screen.getByRole('button', { name: /older/i }));
    await waitFor(() =>
      expect(screen.getByTestId('location')).toHaveTextContent('/audit?outcome=denied&cursor=120&prev=-'),
    );
    await waitFor(() => expect(screen.getByRole('button', { name: /older/i })).toBeDisabled());

    await userEvent.click(screen.getByRole('button', { name: /newer/i }));
    await waitFor(() => expect(screen.getByTestId('location')).toHaveTextContent(/^\/audit\?outcome=denied$/));
  });

  it('applies filters from the form', async () => {
    mockApi(common);
    renderPage(<AuditPage />, { route: '/audit' });
    await screen.findByText('ci-deploy');
    await userEvent.type(screen.getByLabelText('Principal'), 'alice');
    await userEvent.click(screen.getByRole('button', { name: 'Apply filters' }));
    await waitFor(() => expect(screen.getByTestId('location')).toHaveTextContent('/audit?principal=alice'));
  });
});
