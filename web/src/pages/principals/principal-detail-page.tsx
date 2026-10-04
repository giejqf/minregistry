import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { Ban, KeyRound, ShieldCheck } from 'lucide-react';
import { Link, useParams, useSearchParams } from 'react-router';
import { toast } from 'sonner';

import { ConfirmDialog } from '@/components/confirm-dialog';
import { PageHeader } from '@/components/page-header';
import { EmptyState, LoadingRows, QueryError } from '@/components/query-state';
import { Timestamp } from '@/components/timestamp';
import { Button } from '@/components/ui/button';
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card';
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from '@/components/ui/table';
import { useAuth } from '@/hooks/use-auth';
import { invalidate } from '@/lib/invalidate';
import { canIssueToken } from '@/lib/principals';
import { IdentityEnabledSwitch } from '@/pages/principals/identity-enabled-switch';
import { IssueTokenDialog } from '@/pages/principals/issue-token-dialog';
import { KindBadge, LevelBadge, PrincipalStatusBadge, TokenStatusBadge } from '@/pages/principals/status-badges';
import {
  getMeQueryKey,
  getPrincipalOptions,
  getPrincipalQueryKey,
  listPrincipalsQueryKey,
  listTokensQueryKey,
  revokeTokenMutation,
} from '@/sdk/@tanstack/react-query.gen';
import type { PrincipalDetail, TokenSummary } from '@/sdk/types.gen';

function TokensCard({
  detail,
  canIssue,
  onIssue,
}: {
  detail: PrincipalDetail;
  canIssue: boolean;
  onIssue: () => void;
}) {
  const queryClient = useQueryClient();
  const principal = detail.principal;
  const revoke = useMutation({
    ...revokeTokenMutation(),
    onSuccess: () => {
      toast.success('Token revoked.');
      return invalidate(
        queryClient,
        getPrincipalQueryKey({ path: { id: principal.id } }),
        listTokensQueryKey({ path: { id: principal.id } }),
        listPrincipalsQueryKey(),
        getMeQueryKey(),
      );
    },
  });
  const tokens = [...detail.tokens].sort(
    (a, b) => Number(b.status === 'active') - Number(a.status === 'active') || b.created_at.localeCompare(a.created_at),
  );

  return (
    <Card>
      <CardHeader>
        <CardTitle>Tokens</CardTitle>
        <CardDescription>
          Passwords for <code>docker login -u {principal.name}</code>. Secrets are shown only once, when issued.
        </CardDescription>
      </CardHeader>
      <CardContent className={tokens.length ? 'px-0' : undefined}>
        {tokens.length === 0 ? (
          <EmptyState
            icon={KeyRound}
            title="No tokens"
            description="This principal cannot use the registry API until it has a token."
          >
            {canIssue && (
              <Button onClick={onIssue}>
                <KeyRound /> Issue token
              </Button>
            )}
          </EmptyState>
        ) : (
          <Table>
            <TableHeader>
              <TableRow>
                <TableHead className="pl-4">Name</TableHead>
                <TableHead>Prefix</TableHead>
                <TableHead>Status</TableHead>
                <TableHead>Created</TableHead>
                <TableHead>Last used</TableHead>
                <TableHead>Expires</TableHead>
                <TableHead className="w-0 pr-4">
                  <span className="sr-only">Actions</span>
                </TableHead>
              </TableRow>
            </TableHeader>
            <TableBody>
              {tokens.map((token: TokenSummary) => (
                <TableRow key={token.id}>
                  <TableCell className="pl-4 font-medium">{token.name}</TableCell>
                  <TableCell>
                    <code className="font-mono text-xs">{token.prefix}…</code>
                  </TableCell>
                  <TableCell>
                    <TokenStatusBadge status={token.status} />
                  </TableCell>
                  <TableCell>
                    <Timestamp value={token.created_at} />
                    {token.created_by && <div className="text-xs text-muted-foreground">by {token.created_by}</div>}
                  </TableCell>
                  <TableCell>
                    <Timestamp value={token.last_used_at} empty="Never" />
                  </TableCell>
                  <TableCell>
                    {token.status === 'revoked' ? (
                      <span className="text-muted-foreground">
                        Revoked <Timestamp value={token.revoked_at} />
                      </span>
                    ) : (
                      <Timestamp value={token.expires_at} empty="Never" />
                    )}
                  </TableCell>
                  <TableCell className="pr-4">
                    {token.status !== 'revoked' && (
                      <ConfirmDialog
                        trigger={
                          <Button variant="ghost" size="sm" aria-label={`Revoke token ${token.name}`}>
                            <Ban /> Revoke
                          </Button>
                        }
                        title={`Revoke token “${token.name}”?`}
                        description={
                          <p>
                            Clients using the token <code>{token.prefix}…</code> of <strong>{principal.name}</strong>{' '}
                            are rejected immediately. This cannot be undone.
                          </p>
                        }
                        confirmLabel="Revoke token"
                        onConfirm={() => revoke.mutateAsync({ path: { id: principal.id, token_id: token.id } })}
                      />
                    )}
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

function GrantsCard({ detail }: { detail: PrincipalDetail }) {
  const grants = [...detail.permissions].sort((a, b) => a.repository.name.localeCompare(b.repository.name));
  const isAdmin = detail.principal.is_admin;
  return (
    <Card>
      <CardHeader>
        <CardTitle>Repository access</CardTitle>
        <CardDescription>
          {isAdmin
            ? 'As a GitHub admin this principal can access every repository, regardless of the grants below.'
            : 'Per-repository grants. Manage them from each repository’s permissions page.'}
        </CardDescription>
      </CardHeader>
      <CardContent className={grants.length ? 'px-0' : undefined}>
        {grants.length === 0 ? (
          <p className="text-sm text-muted-foreground">No repository grants.</p>
        ) : (
          <Table>
            <TableHeader>
              <TableRow>
                <TableHead className="pl-4">Repository</TableHead>
                <TableHead>Level</TableHead>
                <TableHead>Granted</TableHead>
                <TableHead className="w-0 pr-4">
                  <span className="sr-only">Actions</span>
                </TableHead>
              </TableRow>
            </TableHeader>
            <TableBody>
              {grants.map((g) => (
                <TableRow key={g.repository.id}>
                  <TableCell className="pl-4 font-medium">
                    <Link to={`/repositories/${g.repository.id}`} className="hover:underline">
                      {g.repository.name}
                    </Link>
                  </TableCell>
                  <TableCell>
                    <LevelBadge level={g.level} />
                  </TableCell>
                  <TableCell>
                    <Timestamp value={g.granted_at} />
                    {g.granted_by && <span className="text-muted-foreground"> by {g.granted_by}</span>}
                  </TableCell>
                  <TableCell className="pr-4">
                    <Button variant="ghost" size="sm" asChild>
                      <Link to={`/repositories/${g.repository.id}/permissions`}>
                        <ShieldCheck /> Manage
                      </Link>
                    </Button>
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

function PrincipalView({ detail }: { detail: PrincipalDetail }) {
  const { me } = useAuth();
  const [params, setParams] = useSearchParams();
  const principal = detail.principal;
  const canIssue = canIssueToken(principal, me);
  const isMe = principal.id === me.id;
  const issueOpen = canIssue && params.get('issue') === '1';
  const setIssueOpen = (open: boolean) =>
    setParams(
      (prev) => {
        const next = new URLSearchParams(prev);
        if (open) next.set('issue', '1');
        else next.delete('issue');
        return next;
      },
      { replace: true },
    );

  return (
    <>
      <PageHeader
        eyebrow={
          <Link to="/principals" className="hover:underline">
            Principals
          </Link>
        }
        title={
          <span className="flex flex-wrap items-center gap-2">
            {principal.name}
            <KindBadge kind={principal.kind} />
            <PrincipalStatusBadge principal={principal} />
          </span>
        }
        description={
          <>
            {principal.display_name !== principal.name && <>{principal.display_name} · </>}
            Created <Timestamp value={principal.created_at} />
            {principal.kind === 'github' && !principal.is_admin && (
              <> · No longer listed in MINREGISTRY_ADMIN_GITHUB_LOGINS, so it cannot sign in or use its tokens.</>
            )}
          </>
        }
        actions={
          <>
            {principal.kind === 'identity' && (
              <label className="flex items-center gap-2 text-sm">
                <IdentityEnabledSwitch identity={principal} />
                Enabled
              </label>
            )}
            {canIssue && (
              <Button onClick={() => setIssueOpen(true)}>
                <KeyRound /> {isMe ? 'Create my CLI token' : 'Issue token'}
              </Button>
            )}
          </>
        }
      />
      {principal.kind === 'github' && !isMe && (
        <p className="text-sm text-muted-foreground">
          Only {principal.name} can create tokens for this GitHub account (from their own session). You can still revoke
          them.
        </p>
      )}
      <TokensCard detail={detail} canIssue={canIssue} onIssue={() => setIssueOpen(true)} />
      <GrantsCard detail={detail} />
      {canIssue && (
        <IssueTokenDialog
          principal={principal}
          open={issueOpen}
          onOpenChange={setIssueOpen}
          suggestedName={isMe ? 'cli' : ''}
        />
      )}
    </>
  );
}

export function PrincipalDetailPage() {
  const { id = '' } = useParams();
  const query = useQuery(getPrincipalOptions({ path: { id } }));
  if (query.isPending) return <LoadingRows rows={6} />;
  if (query.isError)
    return <QueryError error={query.error} onRetry={() => void query.refetch()} title="Could not load the principal" />;
  return <PrincipalView detail={query.data} />;
}
