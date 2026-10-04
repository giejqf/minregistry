import type { ManifestSummary, TagSummary } from '@/sdk/types.gen';

export interface ReferrerGroup {
  /** The digest the referrers point at (their `subject`). */
  subject: string;
  /** The subject manifest, when it is stored in this repository. */
  subjectManifest: ManifestSummary | undefined;
  referrers: ManifestSummary[];
}

export interface ManifestOverview {
  /** Manifests that are not referrers (images, indexes, plain artifacts). */
  manifests: ManifestSummary[];
  /** Referrers grouped by subject, in the order of `manifests`, then dangling subjects. */
  referrerGroups: ReferrerGroup[];
  /** Number of referrers per subject digest. */
  referrerCounts: Map<string, number>;
}

const newestFirst = (a: ManifestSummary, b: ManifestSummary) =>
  b.created_at.localeCompare(a.created_at) || a.digest.localeCompare(b.digest);

/**
 * Splits a repository's manifests into subjects and referrers (manifests whose
 * `subject_digest` points at another manifest: signatures, SBOMs, attestations).
 */
export function groupManifests(all: readonly ManifestSummary[]): ManifestOverview {
  const byDigest = new Map(all.map((m) => [m.digest, m]));
  const manifests = all.filter((m) => !m.subject_digest).sort(newestFirst);
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
  return { manifests, referrerGroups, referrerCounts };
}

/** The tag a `docker pull` hint should use: `latest` if present, else the most recently updated. */
export function preferredTag(tags: readonly TagSummary[]): TagSummary | undefined {
  return (
    tags.find((t) => t.name === 'latest') ??
    [...tags].sort((a, b) => b.updated_at.localeCompare(a.updated_at) || a.name.localeCompare(b.name))[0]
  );
}
