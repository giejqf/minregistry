import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { ShieldCheck, Trash2 } from 'lucide-react';
import { Link, useParams } from 'react-router';
import { toast } from 'sonner';

import { ConfirmDialog } from '@/components/confirm-dialog';
import { PageHeader } from '@/components/page-header';
import { EmptyState, LoadingRows, QueryError } from '@/components/query-state';
import { Timestamp } from '@/components/timestamp';
import { Button } from '@/components/ui/button';
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card';
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select';
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from '@/components/ui/table';
import { invalidate } from '@/lib/invalidate';
import { LEVEL_DESCRIPTIONS } from '@/lib/permissions';
import { PERMISSION_LEVELS } from '@/lib/schemas';
import { GrantPermissionDialog } from '@/pages/permissions/grant-permission-dialog';
import { KindBadge } from '@/pages/principals/status-badges';
import {
  getPrincipalQueryKey,
  getRepositoryOptions,
  grantPermissionMutation,
  listRepositoryPermissionsOptions,
  listRepositoryPermissionsQueryKey,
  revokePermissionMutation,
} from '@/sdk/@tanstack/react-query.gen';
import type { PermissionLevel, RepositoryPermissionSummary } from '@/sdk/types.gen';

const levelOrder = (level: PermissionLevel) => PERMISSION_LEVELS.indexOf(level);

function GrantsTable({
  repositoryId,
  repositoryName,
  grants,
}: {
  repositoryId: string;
  repositoryName: string;
  grants: RepositoryPermissionSummary[];
}) {
  const queryClient = useQueryClient();
  const refresh = (principalId: string) =>
    invalidate(
      queryClient,
      listRepositoryPermissionsQueryKey({ path: { id: repositoryId } }),
      getPrincipalQueryKey({ path: { id: principalId } }),
    );
  const change = useMutation({
    ...grantPermissionMutation(),
    onSuccess: (p) => {
      toast.success(`${p.principal.name} now has ${p.level} access.`);
      return refresh(p.principal.id);
    },
  });
  const revoke = useMutation({
    ...revokePermissionMutation(),
    onSuccess: (_data, vars) => {
      toast.success('Access revoked.');
      return refresh(vars.path.principal_id);
    },
  });

  const sorted = [...grants].sort(
    (a, b) => levelOrder(b.level) - levelOrder(a.level) || a.principal.name.localeCompare(b.principal.name),
  );

  return (
    <Table>
      <TableHeader>
        <TableRow>
          <TableHead className="pl-4">Principal</TableHead>
          <TableHead className="w-40">Level</TableHead>
          <TableHead>Granted</TableHead>
          <TableHead className="w-0 pr-4">
            <span className="sr-only">Actions</span>
          </TableHead>
        </TableRow>
      </TableHeader>
      <TableBody>
        {sorted.map((grant) => (
          <TableRow key={grant.principal.id}>
            <TableCell className="pl-4">
              <Link to={`/principals/${grant.principal.id}`} className="font-medium hover:underline">
                {grant.principal.name}
              </Link>{' '}
              <KindBadge kind={grant.principal.kind} />
            </TableCell>
            <TableCell>
              <Select
                value={grant.level}
                disabled={change.isPending}
                onValueChange={(level) =>
                  change.mutate({
                    path: { id: repositoryId, principal_id: grant.principal.id },
                    body: { level: level as PermissionLevel },
                  })
                }
              >
                <SelectTrigger size="sm" className="w-32 capitalize" aria-label={`Level for ${grant.principal.name}`}>
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  {PERMISSION_LEVELS.map((level) => (
                    <SelectItem key={level} value={level} className="capitalize" title={LEVEL_DESCRIPTIONS[level]}>
                      {level}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
            </TableCell>
            <TableCell>
              <Timestamp value={grant.granted_at} />
              {grant.granted_by && <span className="text-muted-foreground"> by {grant.granted_by}</span>}
            </TableCell>
            <TableCell className="pr-4">
              <ConfirmDialog
                trigger={
                  <Button variant="ghost" size="icon-sm" aria-label={`Revoke access for ${grant.principal.name}`}>
                    <Trash2 />
                  </Button>
                }
                title={`Revoke ${grant.principal.name}’s access?`}
                description={
                  <p>
                    <strong>{grant.principal.name}</strong> loses its <strong>{grant.level}</strong> access to{' '}
                    <strong>{repositoryName}</strong> immediately.
                  </p>
                }
                confirmLabel="Revoke access"
                onConfirm={() => revoke.mutateAsync({ path: { id: repositoryId, principal_id: grant.principal.id } })}
              />
            </TableCell>
          </TableRow>
        ))}
      </TableBody>
    </Table>
  );
}

export function RepositoryPermissionsPage() {
  const { id = '' } = useParams();
  const repo = useQuery(getRepositoryOptions({ path: { id } }));
  const grants = useQuery(listRepositoryPermissionsOptions({ path: { id } }));
  const name = repo.data?.name;

  if (repo.isError)
    return <QueryError error={repo.error} onRetry={() => void repo.refetch()} title="Could not load the repository" />;

  return (
    <>
      <PageHeader
        eyebrow={
          <span className="flex items-center gap-1">
            <Link to="/permissions" className="hover:underline">
              Permissions
            </Link>
            {name && (
              <>
                <span>/</span>
                <Link to={`/repositories/${id}`} className="hover:underline">
                  {name}
                </Link>
              </>
            )}
          </span>
        }
        title={name ? `${name} permissions` : 'Permissions'}
        description="Who may pull from and push to this repository. GitHub admins always have full access."
        actions={
          name &&
          grants.data && <GrantPermissionDialog repositoryId={id} repositoryName={name} existing={grants.data} />
        }
      />
      <Card>
        <CardHeader>
          <CardTitle>Grants</CardTitle>
          <CardDescription>
            <strong>read</strong>: {LEVEL_DESCRIPTIONS.read} <strong>write</strong>: {LEVEL_DESCRIPTIONS.write}{' '}
            <strong>owner</strong>: {LEVEL_DESCRIPTIONS.owner}
          </CardDescription>
        </CardHeader>
        <CardContent className={grants.data?.length ? 'px-0' : undefined}>
          {grants.isPending || repo.isPending ? (
            <LoadingRows rows={3} />
          ) : grants.isError ? (
            <QueryError error={grants.error} onRetry={() => void grants.refetch()} />
          ) : grants.data.length === 0 ? (
            <EmptyState
              icon={ShieldCheck}
              title="No grants"
              description="Only GitHub admins can access this repository."
            />
          ) : (
            <GrantsTable repositoryId={id} repositoryName={repo.data.name} grants={grants.data} />
          )}
        </CardContent>
      </Card>
    </>
  );
}
