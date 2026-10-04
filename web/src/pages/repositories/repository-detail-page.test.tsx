import { screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, expect, it } from 'vitest';

import { RepositoryDetailPage } from '@/pages/repositories/repository-detail-page';
import { mockApi, renderPage } from '@/test/utils';

const IMAGE = `sha256:${'a'.repeat(64)}`;
const SIG = `sha256:${'b'.repeat(64)}`;

const repo = {
  id: '5',
  name: 'team/app',
  created_at: '2026-10-01T00:00:00Z',
  created_by: 'ci-deploy',
  tags: [{ name: 'latest', digest: IMAGE, updated_at: '2026-10-02T00:00:00Z', updated_by: 'ci-deploy' }],
  manifests: [
    {
      digest: IMAGE,
      media_type: 'application/vnd.oci.image.index.v1+json',
      size: 1536,
      platforms: [
        { os: 'linux', architecture: 'amd64' },
        { os: 'linux', architecture: 'arm64', variant: 'v8' },
      ],
      child_digests: [],
      annotations: {},
      tags: ['latest'],
      created_at: '2026-10-02T00:00:00Z',
      pushed_by: 'ci-deploy',
    },
    {
      digest: SIG,
      media_type: 'application/vnd.oci.image.manifest.v1+json',
      size: 700,
      platforms: [],
      child_digests: [],
      annotations: {},
      tags: [],
      artifact_type: 'application/vnd.dev.sigstore.bundle.v0.3+json',
      subject_digest: IMAGE,
      created_at: '2026-10-02T01:00:00Z',
      pushed_by: 'signer',
    },
  ],
};

describe('RepositoryDetailPage', () => {
  it('shows tags, manifests with platforms, referrers and a pull hint', async () => {
    mockApi({ 'GET /api/v1/repositories/5': { body: repo } });
    renderPage(<RepositoryDetailPage />, { route: '/repositories/5', path: '/repositories/:id' });

    expect(await screen.findByRole('heading', { name: 'team/app' })).toBeInTheDocument();
    expect(screen.getByText(`docker pull ${window.location.host}/team/app:latest`)).toBeInTheDocument();
    expect(screen.getByText('linux/arm64/v8')).toBeInTheDocument();
    expect(screen.getAllByText('OCI index')).toHaveLength(2); // manifest row + referrer subject
    expect(screen.getByText('1.5 KiB')).toBeInTheDocument();
    expect(screen.getByText('1 referrer')).toBeInTheDocument();
    expect(screen.getByText('application/vnd.dev.sigstore.bundle.v0.3+json')).toBeInTheDocument();
    expect(screen.getByRole('link', { name: /permissions/i })).toHaveAttribute('href', '/repositories/5/permissions');
  });

  it('nests the platform images of an index under it', async () => {
    const AMD = `sha256:${'c'.repeat(64)}`;
    const WIN = `sha256:${'d'.repeat(64)}`;
    const image = (digest: string, platform: { os: string; architecture: string; os_version?: string }) => ({
      digest,
      media_type: 'application/vnd.oci.image.manifest.v1+json',
      size: 500,
      platforms: [platform],
      child_digests: [],
      annotations: {},
      tags: [],
      created_at: '2026-10-01T00:00:00Z',
    });
    const multi = {
      ...repo,
      manifests: [
        { ...repo.manifests[0], child_digests: [AMD, WIN] },
        image(AMD, { os: 'linux', architecture: 'amd64' }),
        image(WIN, { os: 'windows', architecture: 'amd64', os_version: '10.0.20348.2655' }),
      ],
    };
    mockApi({ 'GET /api/v1/repositories/5': { body: multi } });
    renderPage(<RepositoryDetailPage />, { route: '/repositories/5', path: '/repositories/:id' });

    const show = await screen.findByRole('button', { name: 'Show 2 platform images' });
    expect(show).toHaveAttribute('aria-expanded', 'false');
    expect(screen.queryByText('10.0.20348.2655')).not.toBeInTheDocument();
    expect(screen.queryByText('untagged')).not.toBeInTheDocument();

    await userEvent.click(show);
    expect(screen.getByRole('button', { name: 'Hide 2 platform images' })).toHaveAttribute('aria-expanded', 'true');
    expect(screen.getByText('10.0.20348.2655')).toBeInTheDocument();
    expect(screen.getByRole('button', { name: `Delete manifest ${WIN.slice(0, 19)}…` })).toBeInTheDocument();
  });

  it('deletes a tag after confirmation', async () => {
    const { requests } = mockApi({
      'GET /api/v1/repositories/5': { body: repo },
      'DELETE /api/v1/repositories/5/tags/latest': { status: 204 },
    });
    renderPage(<RepositoryDetailPage />, { route: '/repositories/5', path: '/repositories/:id' });
    await userEvent.click(await screen.findByRole('button', { name: 'Delete tag latest' }));
    const dialog = await screen.findByRole('alertdialog');
    expect(requests.some((r) => r.method === 'DELETE')).toBe(false);
    await userEvent.click(within(dialog).getByRole('button', { name: 'Delete tag' }));
    await waitFor(() => expect(requests.some((r) => r.method === 'DELETE')).toBe(true));
    const del = requests.find((r) => r.method === 'DELETE');
    expect(del?.url.pathname).toBe('/api/v1/repositories/5/tags/latest');
    expect(del?.headers.get('X-Requested-With')).toBe('XMLHttpRequest');
    await waitFor(() => expect(screen.queryByRole('alertdialog')).not.toBeInTheDocument());
  });

  it('reports a missing repository', async () => {
    mockApi({
      'GET /api/v1/repositories/9': {
        status: 404,
        body: { error: { code: 'not_found', message: 'repository not found' } },
      },
    });
    renderPage(<RepositoryDetailPage />, { route: '/repositories/9', path: '/repositories/:id' });
    expect(await screen.findByText('repository not found')).toBeInTheDocument();
  });
});
