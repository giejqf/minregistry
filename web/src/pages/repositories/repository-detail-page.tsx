import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { FileBox, Link2, ShieldCheck, Tag, Trash2 } from 'lucide-react';
import { useNavigate, useParams, Link } from 'react-router';
import { toast } from 'sonner';

import { CommandSnippet } from '@/components/command-snippet';
import { ConfirmDialog } from '@/components/confirm-dialog';
import { Digest } from '@/components/digest';
import { PageHeader } from '@/components/page-header';
import { EmptyState, LoadingRows, QueryError } from '@/components/query-state';
import { Timestamp } from '@/components/timestamp';
import { Badge } from '@/components/ui/badge';
import { Button } from '@/components/ui/button';
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card';
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from '@/components/ui/table';
import { invalidate } from '@/lib/invalidate';
import {
  displayArtifactType,
  formatBytes,
  mediaTypeLabel,
  platformLabel,
  pullReference,
  registryHost,
  shortDigest,
} from '@/lib/format';
import { groupManifests, preferredTag, type ReferrerGroup } from '@/lib/manifests';
import {
  deleteRepositoryManifestMutation,
  deleteRepositoryMutation,
  deleteRepositoryTagMutation,
  getRepositoryOptions,
  getRepositoryQueryKey,
  getSystemQueryKey,
  listRepositoriesQueryKey,
} from '@/sdk/@tanstack/react-query.gen';
import type { ManifestSummary, RepositoryDetail, TagSummary } from '@/sdk/types.gen';

function useRepositoryMutations(repo: RepositoryDetail) {
  const queryClient = useQueryClient();
  const navigate = useNavigate();
  const refresh = () =>
    invalidate(
      queryClient,
      getRepositoryQueryKey({ path: { id: repo.id } }),
      listRepositoriesQueryKey(),
      getSystemQueryKey(),
    );

  const deleteTag = useMutation({
    ...deleteRepositoryTagMutation(),
    onSuccess: (_data, vars) => {
      toast.success(`Tag ${vars.path.tag} deleted.`);
      return refresh();
    },
  });
  const deleteManifest = useMutation({
    ...deleteRepositoryManifestMutation(),
    onSuccess: (_data, vars) => {
      toast.success(`Manifest ${shortDigest(vars.path.digest)} deleted.`);
      return refresh();
    },
  });
  const deleteRepo = useMutation({
    ...deleteRepositoryMutation(),
    onSuccess: async () => {
      toast.success(`Repository ${repo.name} deleted.`);
      queryClient.removeQueries({ queryKey: getRepositoryQueryKey({ path: { id: repo.id } }) });
      await invalidate(queryClient, listRepositoriesQueryKey(), getSystemQueryKey());
      await navigate('/repositories');
    },
  });

  return {
    deleteTag: (tag: string) => deleteTag.mutateAsync({ path: { id: repo.id, tag } }),
    deleteManifest: (digest: string) => deleteManifest.mutateAsync({ path: { id: repo.id, digest } }),
    deleteRepository: () => deleteRepo.mutateAsync({ path: { id: repo.id } }),
  };
}

type Mutations = ReturnType<typeof useRepositoryMutations>;

function TagsCard({ repo, mutations }: { repo: RepositoryDetail; mutations: Mutations }) {
  const host = registryHost();
  const tags = [...repo.tags].sort((a, b) => a.name.localeCompare(b.name));
  return (
    <Card>
      <CardHeader>
        <CardTitle>Tags</CardTitle>
        <CardDescription>{tags.length === 1 ? '1 tag' : `${tags.length} tags`}</CardDescription>
      </CardHeader>
      <CardContent className={tags.length ? 'px-0' : undefined}>
        {tags.length === 0 ? (
          <p className="text-sm text-muted-foreground">No tags. Untagged manifests are listed below.</p>
        ) : (
          <Table>
            <TableHeader>
              <TableRow>
                <TableHead className="pl-4">Tag</TableHead>
                <TableHead>Digest</TableHead>
                <TableHead>Updated</TableHead>
                <TableHead>Updated by</TableHead>
                <TableHead className="w-0 pr-4">
                  <span className="sr-only">Actions</span>
                </TableHead>
              </TableRow>
            </TableHeader>
            <TableBody>
              {tags.map((tag: TagSummary) => (
                <TableRow key={tag.name}>
                  <TableCell className="pl-4 font-medium">{tag.name}</TableCell>
                  <TableCell>
                    <Digest digest={tag.digest} />
                  </TableCell>
                  <TableCell>
                    <Timestamp value={tag.updated_at} />
                  </TableCell>
                  <TableCell>{tag.updated_by ?? <span className="text-muted-foreground">—</span>}</TableCell>
                  <TableCell className="pr-4">
                    <ConfirmDialog
                      trigger={
                        <Button variant="ghost" size="icon-sm" aria-label={`Delete tag ${tag.name}`}>
                          <Trash2 />
                        </Button>
                      }
                      title={`Delete tag ${tag.name}?`}
                      description={
                        <p>
                          <code>{pullReference(host, repo.name, tag.name)}</code> will stop resolving. The manifest{' '}
                          <code>{shortDigest(tag.digest)}</code> stays and can still be pulled by digest.
                        </p>
                      }
                      confirmLabel="Delete tag"
                      onConfirm={() => mutations.deleteTag(tag.name)}
                    />
                  </TableCell>
                </TableRow>
              ))}
            </TableBody>
          </Table>
        )}
      </CardContent>
    </Card>
  );
}

function ManifestType({ manifest }: { manifest: ManifestSummary }) {
  const artifactType = displayArtifactType(manifest.artifact_type);
  return (
    <div className="space-y-0.5">
      <div title={manifest.media_type}>{mediaTypeLabel(manifest.media_type)}</div>
      {artifactType && (
        <div className="text-xs break-all text-muted-foreground" title="Artifact type">
          {artifactType}
        </div>
      )}
    </div>
  );
}

function Platforms({ manifest }: { manifest: ManifestSummary }) {
  if (manifest.platforms.length === 0) return <span className="text-muted-foreground">—</span>;
  return (
    <div className="flex max-w-56 flex-wrap gap-1">
      {manifest.platforms.map((p) => {
        const label = platformLabel(p);
        return (
          <Badge key={label} variant="outline" className="font-mono">
            {label}
          </Badge>
        );
      })}
    </div>
  );
}

function DeleteManifestButton({
  repo,
  manifest,
  mutations,
}: {
  repo: RepositoryDetail;
  manifest: ManifestSummary;
  mutations: Mutations;
}) {
  return (
    <ConfirmDialog
      trigger={
        <Button variant="ghost" size="icon-sm" aria-label={`Delete manifest ${shortDigest(manifest.digest)}`}>
          <Trash2 />
        </Button>
      }
      title="Delete this manifest?"
      description={
        <div className="space-y-2">
          <p>
            <code className="break-all">{manifest.digest}</code> will be removed from <strong>{repo.name}</strong>
            {manifest.tags.length > 0 && (
              <>
                {' '}
                together with {manifest.tags.length === 1 ? 'the tag' : 'the tags'}{' '}
                <strong>{manifest.tags.join(', ')}</strong>
              </>
            )}
            .
          </p>
          <p>Unreferenced blobs are reclaimed by the next garbage collection.</p>
        </div>
      }
      confirmLabel="Delete manifest"
      onConfirm={() => mutations.deleteManifest(manifest.digest)}
    />
  );
}

function ManifestsCard({
  repo,
  manifests,
  referrerCounts,
  mutations,
}: {
  repo: RepositoryDetail;
  manifests: ManifestSummary[];
  referrerCounts: Map<string, number>;
  mutations: Mutations;
}) {
  return (
    <Card>
      <CardHeader>
        <CardTitle>Manifests</CardTitle>
        <CardDescription>
          Images, indexes and artifacts stored in this repository (referrers are listed separately).
        </CardDescription>
      </CardHeader>
      <CardContent className={manifests.length ? 'px-0' : undefined}>
        {manifests.length === 0 ? (
          <p className="text-sm text-muted-foreground">No manifests.</p>
        ) : (
          <Table>
            <TableHeader>
              <TableRow>
                <TableHead className="pl-4">Digest</TableHead>
                <TableHead>Type</TableHead>
                <TableHead>Platforms</TableHead>
                <TableHead className="text-right">Size</TableHead>
                <TableHead>Tags</TableHead>
                <TableHead>Pushed</TableHead>
                <TableHead className="w-0 pr-4">
                  <span className="sr-only">Actions</span>
                </TableHead>
              </TableRow>
            </TableHeader>
            <TableBody>
              {manifests.map((m) => {
                const referrers = referrerCounts.get(m.digest) ?? 0;
                return (
                  <TableRow key={m.digest} className="align-top">
                    <TableCell className="pl-4">
                      <Digest digest={m.digest} />
                      {referrers > 0 && (
                        <a href={`#referrers-${m.digest}`} className="mt-1 block w-fit">
                          <Badge variant="secondary">
                            <Link2 /> {referrers === 1 ? '1 referrer' : `${referrers} referrers`}
                          </Badge>
                        </a>
                      )}
                    </TableCell>
                    <TableCell>
                      <ManifestType manifest={m} />
                    </TableCell>
                    <TableCell>
                      <Platforms manifest={m} />
                    </TableCell>
                    <TableCell className="text-right whitespace-nowrap tabular-nums">{formatBytes(m.size)}</TableCell>
                    <TableCell>
                      {m.tags.length ? (
                        <div className="flex max-w-48 flex-wrap gap-1">
                          {m.tags.map((t) => (
                            <Badge key={t} variant="secondary">
                              <Tag /> {t}
                            </Badge>
                          ))}
                        </div>
                      ) : (
                        <span className="text-muted-foreground">untagged</span>
                      )}
                    </TableCell>
                    <TableCell>
                      <Timestamp value={m.created_at} />
                      {m.pushed_by && <div className="text-xs text-muted-foreground">by {m.pushed_by}</div>}
                    </TableCell>
                    <TableCell className="pr-4">
                      <DeleteManifestButton repo={repo} manifest={m} mutations={mutations} />
                    </TableCell>
                  </TableRow>
                );
              })}
            </TableBody>
          </Table>
        )}
      </CardContent>
    </Card>
  );
}

function ReferrerGroupView({
  repo,
  group,
  mutations,
}: {
  repo: RepositoryDetail;
  group: ReferrerGroup;
  mutations: Mutations;
}) {
  const subject = group.subjectManifest;
  return (
    <div id={`referrers-${group.subject}`} className="scroll-mt-4 rounded-lg border">
      <div className="flex flex-wrap items-center gap-2 border-b bg-muted/40 px-4 py-2 text-sm">
        <span className="text-muted-foreground">Subject</span>
        <Digest digest={group.subject} />
        {subject ? (
          <>
            <span className="text-muted-foreground">{mediaTypeLabel(subject.media_type)}</span>
            {subject.tags.map((t) => (
              <Badge key={t} variant="secondary">
                <Tag /> {t}
              </Badge>
            ))}
            {subject.subject_digest && <Badge variant="outline">referrer</Badge>}
          </>
        ) : (
          <Badge variant="outline">not in this repository</Badge>
        )}
      </div>
      <Table>
        <TableHeader>
          <TableRow>
            <TableHead className="pl-4">Referrer</TableHead>
            <TableHead>Artifact type</TableHead>
            <TableHead>Media type</TableHead>
            <TableHead className="text-right">Size</TableHead>
            <TableHead>Pushed</TableHead>
            <TableHead className="w-0 pr-4">
              <span className="sr-only">Actions</span>
            </TableHead>
          </TableRow>
        </TableHeader>
        <TableBody>
          {group.referrers.map((r) => (
            <TableRow key={r.digest}>
              <TableCell className="pl-4">
                <Digest digest={r.digest} />
              </TableCell>
              <TableCell className="break-all">
                {r.artifact_type ?? <span className="text-muted-foreground">—</span>}
              </TableCell>
              <TableCell title={r.media_type}>{mediaTypeLabel(r.media_type)}</TableCell>
              <TableCell className="text-right whitespace-nowrap tabular-nums">{formatBytes(r.size)}</TableCell>
              <TableCell>
                <Timestamp value={r.created_at} />
                {r.pushed_by && <div className="text-xs text-muted-foreground">by {r.pushed_by}</div>}
              </TableCell>
              <TableCell className="pr-4">
                <DeleteManifestButton repo={repo} manifest={r} mutations={mutations} />
              </TableCell>
            </TableRow>
          ))}
        </TableBody>
      </Table>
    </div>
  );
}

function ReferrersCard({
  repo,
  groups,
  mutations,
}: {
  repo: RepositoryDetail;
  groups: ReferrerGroup[];
  mutations: Mutations;
}) {
  if (groups.length === 0) return null;
  return (
    <Card>
      <CardHeader>
        <CardTitle>Referrers</CardTitle>
        <CardDescription>
          Signatures, SBOMs, attestations and other artifacts attached to a manifest, grouped by subject.
        </CardDescription>
      </CardHeader>
      <CardContent className="space-y-4">
        {groups.map((group) => (
          <ReferrerGroupView key={group.subject} repo={repo} group={group} mutations={mutations} />
        ))}
      </CardContent>
    </Card>
  );
}

function RepositoryView({ repo }: { repo: RepositoryDetail }) {
  const mutations = useRepositoryMutations(repo);
  const { manifests, referrerGroups, referrerCounts } = groupManifests(repo.manifests);
  const host = registryHost();
  const tag = preferredTag(repo.tags);
  const pull = `docker pull ${pullReference(host, repo.name, tag?.name ?? 'latest')}`;

  return (
    <>
      <PageHeader
        eyebrow={
          <Link to="/repositories" className="hover:underline">
            Repositories
          </Link>
        }
        title={repo.name}
        description={
          <>
            Created <Timestamp value={repo.created_at} />
            {repo.created_by && <> by {repo.created_by}</>} · {repo.tags.length} tags · {repo.manifests.length}{' '}
            manifests
          </>
        }
        actions={
          <>
            <Button variant="outline" asChild>
              <Link to={`/repositories/${repo.id}/permissions`}>
                <ShieldCheck /> Permissions
              </Link>
            </Button>
            <ConfirmDialog
              trigger={
                <Button variant="destructive">
                  <Trash2 /> Delete repository
                </Button>
              }
              title={`Delete ${repo.name}?`}
              description={
                <div className="space-y-2">
                  <p>
                    All {repo.tags.length} tags, {repo.manifests.length} manifests and every permission grant of this
                    repository are deleted. In-flight uploads are discarded.
                  </p>
                  <p>Blobs are reclaimed by the next garbage collection. This cannot be undone.</p>
                </div>
              }
              confirmLabel="Delete repository"
              onConfirm={mutations.deleteRepository}
            />
          </>
        }
      />
      <CommandSnippet command={pull} label="pull command" />
      {repo.manifests.length === 0 ? (
        <EmptyState icon={FileBox} title="This repository is empty" description="It has no tags or manifests." />
      ) : (
        <>
          <TagsCard repo={repo} mutations={mutations} />
          <ManifestsCard repo={repo} manifests={manifests} referrerCounts={referrerCounts} mutations={mutations} />
          <ReferrersCard repo={repo} groups={referrerGroups} mutations={mutations} />
        </>
      )}
    </>
  );
}

export function RepositoryDetailPage() {
  const { id = '' } = useParams();
  const query = useQuery(getRepositoryOptions({ path: { id } }));
  if (query.isPending) return <LoadingRows rows={6} />;
  if (query.isError)
    return (
      <QueryError error={query.error} onRetry={() => void query.refetch()} title="Could not load the repository" />
    );
  return <RepositoryView repo={query.data} />;
}
