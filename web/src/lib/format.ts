import type { PlatformSummary } from '@/sdk/types.gen';

const BYTE_UNITS = ['B', 'KiB', 'MiB', 'GiB', 'TiB', 'PiB'] as const;

/** 1536 → "1.5 KiB" (binary units, at most one decimal). */
export function formatBytes(bytes: number): string {
  if (!Number.isFinite(bytes) || bytes < 0) return '—';
  let value = bytes;
  let unit = 0;
  while (value >= 1024 && unit < BYTE_UNITS.length - 1) {
    value /= 1024;
    unit += 1;
  }
  if (unit === 0) return `${value} B`;
  const rounded = value >= 100 ? Math.round(value).toString() : value.toFixed(1).replace(/\.0$/, '');
  return `${rounded} ${BYTE_UNITS[unit]}`;
}

/** "sha256:0123456789abcdef…" → "sha256:0123456789ab…". */
export function shortDigest(digest: string, length = 12): string {
  const colon = digest.indexOf(':');
  const algorithm = colon >= 0 ? digest.slice(0, colon + 1) : '';
  const hex = colon >= 0 ? digest.slice(colon + 1) : digest;
  return hex.length > length ? `${algorithm}${hex.slice(0, length)}…` : digest;
}

function parse(iso: string | null | undefined): Date | null {
  if (!iso) return null;
  const date = new Date(iso);
  return Number.isNaN(date.getTime()) ? null : date;
}

/** Local date and time, e.g. "Oct 3, 2026, 19:59:01". */
export function formatDateTime(iso: string | null | undefined): string {
  const date = parse(iso);
  if (!date) return '—';
  return date.toLocaleString(undefined, {
    year: 'numeric',
    month: 'short',
    day: 'numeric',
    hour: '2-digit',
    minute: '2-digit',
    second: '2-digit',
  });
}

const RELATIVE_STEPS: ReadonlyArray<[Intl.RelativeTimeFormatUnit, number]> = [
  ['second', 60],
  ['minute', 60],
  ['hour', 24],
  ['day', 30],
  ['month', 12],
  ['year', Number.POSITIVE_INFINITY],
];

/** "5 minutes ago", "in 3 days", "just now". */
export function formatRelative(iso: string | null | undefined, now: Date = new Date()): string {
  const date = parse(iso);
  if (!date) return '—';
  let delta = (date.getTime() - now.getTime()) / 1000;
  if (Math.abs(delta) < 45) return 'just now';
  const rtf = new Intl.RelativeTimeFormat(undefined, { numeric: 'auto' });
  for (const [unit, size] of RELATIVE_STEPS) {
    if (Math.abs(delta) < size) return rtf.format(Math.round(delta), unit);
    delta /= size;
  }
  return formatDateTime(iso);
}

/** 90 → "1m 30s", 86400 → "1d", 0 → "0s". */
export function formatDuration(totalSeconds: number): string {
  if (!Number.isFinite(totalSeconds) || totalSeconds < 0) return '—';
  const units: ReadonlyArray<[string, number]> = [
    ['d', 86_400],
    ['h', 3_600],
    ['m', 60],
    ['s', 1],
  ];
  let rest = Math.floor(totalSeconds);
  const parts: string[] = [];
  for (const [label, size] of units) {
    const n = Math.floor(rest / size);
    if (n > 0) {
      parts.push(`${n}${label}`);
      rest -= n * size;
    }
    if (parts.length === 2) break;
  }
  return parts.length ? parts.join(' ') : '0s';
}

/** 1234 → "1.2 s", 87 → "87 ms". */
export function formatMillis(ms: number): string {
  return ms < 1000 ? `${ms} ms` : `${(ms / 1000).toFixed(1)} s`;
}

/** {os: linux, architecture: arm64, variant: v8} → "linux/arm64/v8" (without the OS version). */
export function platformLabel(p: PlatformSummary): string {
  return [p.os, p.architecture, p.variant].filter(Boolean).join('/');
}

/** The platform label with its OS version, e.g. "windows/amd64 10.0.20348.2655". */
export function platformTitle(p: PlatformSummary): string {
  return p.os_version ? `${platformLabel(p)} ${p.os_version}` : platformLabel(p);
}

const MEDIA_TYPES: Record<string, string> = {
  'application/vnd.oci.image.index.v1+json': 'OCI index',
  'application/vnd.oci.image.manifest.v1+json': 'OCI manifest',
  'application/vnd.oci.artifact.manifest.v1+json': 'OCI artifact',
  'application/vnd.docker.distribution.manifest.list.v2+json': 'Docker manifest list',
  'application/vnd.docker.distribution.manifest.v2+json': 'Docker manifest',
  'application/vnd.docker.distribution.manifest.v1+prettyjws': 'Docker manifest v1',
};

/** A short name for well-known manifest media types; the raw type otherwise. */
export function mediaTypeLabel(mediaType: string): string {
  return MEDIA_TYPES[mediaType] ?? mediaType;
}

const IMAGE_CONFIG_TYPES = new Set([
  'application/vnd.oci.image.config.v1+json',
  'application/vnd.docker.container.image.v1+json',
]);

/** The artifact type worth showing: none for plain container images (whose "type" is their config's). */
export function displayArtifactType(artifactType: string | null | undefined): string | null {
  return artifactType && !IMAGE_CONFIG_TYPES.has(artifactType) ? artifactType : null;
}

export function formatCount(n: number): string {
  return n.toLocaleString();
}

/** `<input type="datetime-local">` value (local time) for an RFC 3339 timestamp. */
export function toLocalInputValue(iso: string | null | undefined): string {
  const date = parse(iso);
  if (!date) return '';
  const pad = (n: number) => n.toString().padStart(2, '0');
  return (
    `${date.getFullYear()}-${pad(date.getMonth() + 1)}-${pad(date.getDate())}` +
    `T${pad(date.getHours())}:${pad(date.getMinutes())}`
  );
}

/** RFC 3339 (UTC) for a `datetime-local` value; '' when empty or invalid. */
export function fromLocalInputValue(value: string): string {
  if (!value) return '';
  const date = new Date(value);
  return Number.isNaN(date.getTime()) ? '' : date.toISOString();
}

/** The registry host clients use: the host the UI is served from. */
export function registryHost(): string {
  return window.location.host;
}

/** `docker pull` reference: `host/name:tag` or `host/name@sha256:…`. */
export function pullReference(host: string, repository: string, reference: string): string {
  return reference.includes(':') ? `${host}/${repository}@${reference}` : `${host}/${repository}:${reference}`;
}

/** The `docker login` command for a freshly issued token. */
export function dockerLoginSnippet(host: string, username: string, secret: string): string {
  return `echo '${secret}' | docker login ${host} -u ${username} --password-stdin`;
}
