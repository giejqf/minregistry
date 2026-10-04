import { render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, expect, it, vi } from 'vitest';

import { TokenSecretDialog } from '@/pages/principals/token-secret-dialog';

const created = {
  secret: 'mr_s3cr3tvalue',
  username: 'ci-deploy',
  token: {
    id: '9',
    name: 'github-actions',
    prefix: 'mr_s3cr3',
    status: 'active' as const,
    created_at: '2026-10-03T00:00:00Z',
  },
};

describe('TokenSecretDialog', () => {
  it('shows the secret once with a docker login snippet', async () => {
    const onClose = vi.fn();
    render(<TokenSecretDialog created={created} onClose={onClose} />);

    expect(screen.getByText(/you won’t see it again/i)).toBeInTheDocument();
    expect(screen.getByText('mr_s3cr3tvalue')).toBeInTheDocument();
    expect(
      screen.getByText(`echo 'mr_s3cr3tvalue' | docker login ${window.location.host} -u ci-deploy --password-stdin`),
    ).toBeInTheDocument();

    const writeText = vi.fn(() => Promise.resolve());
    Object.defineProperty(navigator, 'clipboard', { value: { writeText }, configurable: true });
    await userEvent.click(screen.getByRole('button', { name: 'Copy token' }));
    expect(writeText).toHaveBeenCalledWith('mr_s3cr3tvalue');

    await userEvent.click(screen.getByRole('button', { name: 'Done' }));
    expect(onClose).toHaveBeenCalled();
  });

  it('renders nothing without a token', () => {
    render(<TokenSecretDialog created={null} onClose={() => undefined} />);
    expect(screen.queryByRole('dialog')).not.toBeInTheDocument();
  });
});
