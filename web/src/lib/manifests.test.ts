import { describe, expect, it } from 'vitest';

import { groupManifests, preferredTag } from '@/lib/manifests';
import type { ManifestSummary } from '@/sdk/types.gen';

function manifest(digest: string, overrides: Partial<ManifestSummary> = {}): ManifestSummary {
  return {
    digest,
    media_type: 'application/vnd.oci.image.manifest.v1+json',
    size: 100,
    platforms: [],
    child_digests: [],
    annotations: {},
    tags: [],
    created_at: '2026-10-01T00:00:00Z',
    ...overrides,
  };
}

describe('groupManifests', () => {
  it('separates referrers from their subjects', () => {
    const image = manifest('sha256:image', { tags: ['latest'], created_at: '2026-10-02T00:00:00Z' });
    const old = manifest('sha256:old');
    const sig = manifest('sha256:sig', { subject_digest: 'sha256:image', artifact_type: 'application/vnd.dev.cosign' });
    const sbom = manifest('sha256:sbom', { subject_digest: 'sha256:image', created_at: '2026-10-03T00:00:00Z' });
    const sbomSig = manifest('sha256:sbomsig', { subject_digest: 'sha256:sbom' });
    const dangling = manifest('sha256:dangling', { subject_digest: 'sha256:elsewhere' });

    const result = groupManifests([old, sig, image, sbom, sbomSig, dangling]);

    expect(result.manifests.map((m) => m.digest)).toEqual(['sha256:image', 'sha256:old']);
    expect(result.referrerGroups.map((g) => g.subject)).toEqual(['sha256:image', 'sha256:sbom', 'sha256:elsewhere']);
    expect(result.referrerGroups[0]?.referrers.map((m) => m.digest)).toEqual(['sha256:sbom', 'sha256:sig']);
    expect(result.referrerGroups[0]?.subjectManifest).toBe(image);
    expect(result.referrerGroups[2]?.subjectManifest).toBeUndefined();
    expect(result.referrerCounts.get('sha256:image')).toBe(2);
    expect(result.referrerCounts.get('sha256:old')).toBeUndefined();
  });

  it('nests the untagged platform images of an index under it', () => {
    const arm = manifest('sha256:arm', { platforms: [{ os: 'linux', architecture: 'arm64' }] });
    const amd = manifest('sha256:amd', { platforms: [{ os: 'linux', architecture: 'amd64' }] });
    const win = manifest('sha256:win', {
      platforms: [{ os: 'windows', architecture: 'amd64', os_version: '10.0.20348.2655' }],
      tags: ['windows'],
    });
    const index = manifest('sha256:index', {
      media_type: 'application/vnd.oci.image.index.v1+json',
      child_digests: ['sha256:win', 'sha256:arm', 'sha256:amd', 'sha256:missing'],
      tags: ['latest'],
      created_at: '2026-10-02T00:00:00Z',
    });
    const loose = manifest('sha256:loose');

    const result = groupManifests([arm, amd, win, index, loose]);

    // Tagged platform images stay listed on their own as well.
    expect(result.manifests.map((m) => m.digest)).toEqual(['sha256:index', 'sha256:loose', 'sha256:win']);
    expect(result.children.get('sha256:index')?.map((m) => m.digest)).toEqual([
      'sha256:amd',
      'sha256:arm',
      'sha256:win',
    ]);
    expect(result.children.has('sha256:loose')).toBe(false);
  });

  it('handles repositories without referrers', () => {
    const result = groupManifests([manifest('sha256:a')]);
    expect(result.referrerGroups).toEqual([]);
    expect(result.manifests).toHaveLength(1);
  });
});

describe('preferredTag', () => {
  const tag = (name: string, updated_at: string) => ({ name, digest: 'sha256:x', updated_at });
  it('prefers latest, then the most recently updated tag', () => {
    expect(preferredTag([tag('v1', '2026-01-02T00:00:00Z'), tag('latest', '2026-01-01T00:00:00Z')])?.name).toBe(
      'latest',
    );
    expect(preferredTag([tag('v1', '2026-01-01T00:00:00Z'), tag('v2', '2026-01-02T00:00:00Z')])?.name).toBe('v2');
    expect(preferredTag([])).toBeUndefined();
  });
});
