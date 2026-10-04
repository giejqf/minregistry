import { platformLabel } from '@/lib/format';
import type { ManifestSummary, TagSummary } from '@/sdk/types.gen';

export interface ReferrerGroup {
  /** The digest the referrers point at (their `subject`). */
  subject: string;
  /** The subject manifest, when it is stored in this repository. */
  subjectManifest: ManifestSummary | undefined;
  referrers: ManifestSummary[];
}

export interface ManifestOverview {
  /**
   * Top-level manifests: images, indexes and plain artifacts that are not
   * referrers. Untagged platform images of an index are nested under it
   * (`children`) instead of being listed here.
   */
  manifests: ManifestSummary[];
  /** The platform images of each index stored in this repository, by index digest. */
  children: Map<string, ManifestSummary[]>;
  /** Referrers grouped by subject, in the order of `manifests`, then dangling subjects. */
  referrerGroups: ReferrerGroup[];
  /** Number of referrers per subject digest. */
  referrerCounts: Map<string, number>;
}

const newestFirst = (a: ManifestSummary, b: ManifestSummary) =>
  b.created_at.localeCompare(a.created_at) || a.digest.localeCompare(b.digest);

const platformSortKey = (m: ManifestSummary) =>
  m.platforms.map((p) => `${platformLabel(p)} ${p.os_version ?? ''}`).join(',') || '\uffff';

const byPlatform = (a: ManifestSummary, b: ManifestSummary) =>
  platformSortKey(a).localeCompare(platformSortKey(b)) || a.digest.localeCompare(b.digest);

/**
 * Splits a repository's manifests into subjects and referrers (manifests whose
 * `subject_digest` points at another manifest: signatures, SBOMs, attestations).
 */
export function groupManifests(all: readonly ManifestSummary[]): ManifestOverview {
  const byDigest = new Map(all.map((m) => [m.digest, m]));
  const children = new Map<string, ManifestSummary[]>();
  const nested = new Set<string>();
  for (const index of all) {
    const list = index.child_digests
      .filter((d) => d !== index.digest)
      .map((d) => byDigest.get(d))
      .filter((m): m is ManifestSummary => m !== undefined);
    if (list.length === 0) continue;
    children.set(index.digest, list.sort(byPlatform));
    for (const child of list) nested.add(child.digest);
  }
  // A tagged platform image is also listed on its own, since it can be pulled by that tag.
  const manifests = all
    .filter((m) => !m.subject_digest && !(nested.has(m.digest) && m.tags.length === 0))
    .sort(newestFirst);
  const bySubject = new Map<string, ManifestSummary[]>();
  for (const m of all) {
    if (!m.subject_digest) continue;
    const list = bySubject.get(m.subject_digest) ?? [];
    list.push(m);
    bySubject.set(m.subject_digest, list);
  }
  // Referrers can themselves be subjects (a signature of an SBOM); list their
  // groups after the top-level ones, then subjects missing from the repository.
  const order = [
    ...manifests.map((m) => m.digest),
    ...all
      .filter((m) => m.subject_digest)
      .sort(newestFirst)
      .map((m) => m.digest),
    ...[...bySubject.keys()].sort(),
  ];
  const seen = new Set<string>();
  const referrerGroups: ReferrerGroup[] = [];
  for (const subject of order) {
    const referrers = bySubject.get(subject);
    if (!referrers || seen.has(subject)) continue;
    seen.add(subject);
    referrerGroups.push({ subject, subjectManifest: byDigest.get(subject), referrers: referrers.sort(newestFirst) });
  }
  const referrerCounts = new Map([...bySubject].map(([subject, list]) => [subject, list.length]));
  return { manifests, children, referrerGroups, referrerCounts };
}

/** The tag a `docker pull` hint should use: `latest` if present, else the most recently updated. */
export function preferredTag(tags: readonly TagSummary[]): TagSummary | undefined {
  return (
    tags.find((t) => t.name === 'latest') ??
    [...tags].sort((a, b) => b.updated_at.localeCompare(a.updated_at) || a.name.localeCompare(b.name))[0]
  );
}
