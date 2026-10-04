import { describe, expect, it } from 'vitest';

import {
  displayArtifactType,
  dockerLoginSnippet,
  formatBytes,
  formatDuration,
  formatRelative,
  fromLocalInputValue,
  mediaTypeLabel,
  platformLabel,
  pullReference,
  shortDigest,
  toLocalInputValue,
} from '@/lib/format';

describe('formatBytes', () => {
  it.each([
    [0, '0 B'],
    [512, '512 B'],
    [1024, '1 KiB'],
    [1536, '1.5 KiB'],
    [1048576, '1 MiB'],
    [5.25 * 1024 ** 3, '5.3 GiB'],
    [150 * 1024 ** 2, '150 MiB'],
  ])('%d → %s', (bytes, expected) => {
    expect(formatBytes(bytes)).toBe(expected);
  });

  it('rejects nonsense', () => {
    expect(formatBytes(-1)).toBe('—');
    expect(formatBytes(Number.NaN)).toBe('—');
  });
});

describe('shortDigest', () => {
  const digest = `sha256:${'0123456789abcdef'.repeat(4)}`;
  it('keeps the algorithm and 12 hex characters', () => {
    expect(shortDigest(digest)).toBe('sha256:0123456789ab…');
  });
  it('leaves short values alone', () => {
    expect(shortDigest('sha256:abc')).toBe('sha256:abc');
  });
});

describe('formatRelative', () => {
  const now = new Date('2026-10-03T12:00:00Z');
  it('describes past and future times', () => {
    expect(formatRelative('2026-10-03T11:59:50Z', now)).toBe('just now');
    expect(formatRelative('2026-10-03T11:55:00Z', now)).toMatch(/5 minutes ago/);
    expect(formatRelative('2026-10-01T12:00:00Z', now)).toMatch(/2 days ago/);
    expect(formatRelative('2026-10-10T12:00:00Z', now)).toMatch(/in 7 days/);
  });
  it('handles missing and invalid values', () => {
    expect(formatRelative(null, now)).toBe('—');
    expect(formatRelative('not a date', now)).toBe('—');
  });
});

describe('formatDuration', () => {
  it.each([
    [0, '0s'],
    [90, '1m 30s'],
    [3600, '1h'],
    [86_400, '1d'],
    [90_061, '1d 1h'],
  ])('%d → %s', (seconds, expected) => {
    expect(formatDuration(seconds)).toBe(expected);
  });
});

describe('labels', () => {
  it('formats platforms', () => {
    expect(platformLabel({ os: 'linux', architecture: 'arm64', variant: 'v8' })).toBe('linux/arm64/v8');
    expect(platformLabel({ os: 'linux', architecture: 'amd64', variant: null })).toBe('linux/amd64');
  });
  it('hides the artifact type of plain images', () => {
    expect(displayArtifactType('application/vnd.docker.container.image.v1+json')).toBeNull();
    expect(displayArtifactType('application/vnd.oci.image.config.v1+json')).toBeNull();
    expect(displayArtifactType('application/vnd.example.sbom+json')).toBe('application/vnd.example.sbom+json');
    expect(displayArtifactType(null)).toBeNull();
  });
  it('names well-known media types', () => {
    expect(mediaTypeLabel('application/vnd.oci.image.index.v1+json')).toBe('OCI index');
    expect(mediaTypeLabel('application/x-custom')).toBe('application/x-custom');
  });
});

describe('docker commands', () => {
  it('builds pull references by tag and by digest', () => {
    expect(pullReference('reg.example.com', 'team/app', 'v1')).toBe('reg.example.com/team/app:v1');
    expect(pullReference('reg.example.com', 'team/app', 'sha256:abc')).toBe('reg.example.com/team/app@sha256:abc');
  });
  it('builds the docker login snippet', () => {
    expect(dockerLoginSnippet('localhost:5000', 'ci-deploy', 's3cr3t')).toBe(
      "echo 's3cr3t' | docker login localhost:5000 -u ci-deploy --password-stdin",
    );
  });
});

describe('datetime-local conversion', () => {
  it('round-trips through local time', () => {
    const iso = new Date(2026, 9, 3, 14, 30).toISOString();
    expect(toLocalInputValue(iso)).toBe('2026-10-03T14:30');
    expect(fromLocalInputValue('2026-10-03T14:30')).toBe(iso);
  });
  it('maps empty values to empty strings', () => {
    expect(toLocalInputValue('')).toBe('');
    expect(fromLocalInputValue('')).toBe('');
    expect(fromLocalInputValue('garbage')).toBe('');
  });
});
