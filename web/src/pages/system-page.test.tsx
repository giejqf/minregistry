import { screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, expect, it } from 'vitest';

import { SystemPage } from '@/pages/system-page';
import { mockApi, renderPage, type RecordedRequest } from '@/test/utils';
import type { GcRequest } from '@/sdk/types.gen';

const system = {
  version: '0.1.0',
  started_at: '2026-10-03T00:00:00Z',
  storage: { backend: 'fs', location: '/data/blobs' },
  database: { path: '/data/minregistry.db', size_bytes: 2 * 1024 * 1024 },
  upload_dir: '/data/uploads',
  upload_ttl_seconds: 86_400,
  uploads_in_flight: 1,
  repository_count: 3,
  manifest_count: 12,
  blob_count: 40,
  blob_bytes: 512 * 1024 * 1024,
  gc_cron: null,
  gc_min_age_seconds: 3600,
  audit_retention_days: 0,
  audit_blob_reads: false,
};

function gcReply(req: RecordedRequest) {
  const body = req.body as GcRequest;
  return {
    body: {
      dry_run: body.dry_run ?? false,
      delete_untagged: body.delete_untagged ?? false,
      min_age_seconds: body.min_age_seconds ?? 3600,
      manifests_deleted: 2,
      blobs_deleted: 5,
      bytes_freed: 10 * 1024 * 1024,
      orphans_deleted: 0,
      blobs_kept_young: 1,
      errors: 0,
      duration_ms: 42,
    },
  };
}

describe('SystemPage', () => {
  it('shows storage, database and uploads', async () => {
    mockApi({
      'GET /api/v1/system': { body: system },
      'GET /api/v1/uploads': {
        body: [
          {
            uuid: 'abcdef12-3456',
            repository: 'team/app',
            principal: 'ci',
            offset: 2048,
            started_at: '2026-10-03T00:00:00Z',
            last_activity_at: '2026-10-03T00:01:00Z',
          },
        ],
      },
    });
    renderPage(<SystemPage />);
    expect(await screen.findByText('/data/blobs')).toBeInTheDocument();
    expect(screen.getByText('2 MiB')).toBeInTheDocument();
    expect(screen.getByText('512 MiB')).toBeInTheDocument();
    expect(await screen.findByText('team/app')).toBeInTheDocument();
    expect(screen.getByText('2 KiB')).toBeInTheDocument();
  });

  it('requires a dry run with the same options before running GC for real', async () => {
    const { requests } = mockApi({
      'GET /api/v1/system': { body: system },
      'GET /api/v1/uploads': { body: [] },
      'POST /api/v1/gc': gcReply,
    });
    renderPage(<SystemPage />);
    const run = await screen.findByRole('button', { name: /run garbage collection/i });
    expect(run).toBeDisabled();

    await userEvent.click(screen.getByRole('button', { name: 'Dry run' }));
    expect(await screen.findByText(/dry run — nothing was deleted/i)).toBeInTheDocument();
    expect(requests.at(-1)?.body).toEqual({ dry_run: true, delete_untagged: false, min_age_seconds: 3600 });
    await waitFor(() => expect(run).toBeEnabled());

    // Changing an option invalidates the dry run.
    await userEvent.click(screen.getByLabelText(/delete untagged manifests/i));
    expect(run).toBeDisabled();
    await userEvent.click(screen.getByRole('button', { name: 'Dry run' }));
    await waitFor(() => expect(run).toBeEnabled());

    await userEvent.click(run);
    const dialog = await screen.findByRole('alertdialog');
    expect(within(dialog).getByText(/found 2 manifests and 5 blobs/i)).toBeInTheDocument();
    await userEvent.click(within(dialog).getByRole('button', { name: /run garbage collection/i }));

    expect(await screen.findByText('Completed')).toBeInTheDocument();
    expect(requests.filter((r) => r.method === 'POST').at(-1)?.body).toEqual({
      dry_run: false,
      delete_untagged: true,
      min_age_seconds: 3600,
    });
    expect(run).toBeDisabled();
  });
});
