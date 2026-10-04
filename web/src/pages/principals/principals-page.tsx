import { useQuery } from '@tanstack/react-query';
import { KeyRound, UsersRound } from 'lucide-react';
import { Link } from 'react-router';

import { PageHeader } from '@/components/page-header';
import { EmptyState, LoadingRows, QueryError } from '@/components/query-state';
import { Timestamp } from '@/components/timestamp';
import { Badge } from '@/components/ui/badge';
import { Button } from '@/components/ui/button';
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card';
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from '@/components/ui/table';
import { useAuth } from '@/hooks/use-auth';
import { adminRows, adminStatus, identities } from '@/lib/principals';
import { CreateIdentityDialog } from '@/pages/principals/create-identity-dialog';
import { IdentityEnabledSwitch } from '@/pages/principals/identity-enabled-switch';
import { AdminStatusBadge, PrincipalStatusBadge } from '@/pages/principals/status-badges';
import { listPrincipalsOptions } from '@/sdk/@tanstack/react-query.gen';
import type { PrincipalListResponse } from '@/sdk/types.gen';

function AdminsCard({ list }: { list: PrincipalListResponse }) {
  const { me } = useAuth();
  const rows = adminRows(list);
  return (
    <Card>
      <CardHeader>
        <CardTitle>GitHub admins</CardTitle>
        <CardDescription>
          Configured with <code>MINREGISTRY_ADMIN_GITHUB_LOGINS</code> (read-only here). Admins can sign in to this UI
          and have full access to every repository.
        </CardDescription>
      </CardHeader>
      <CardContent className="px-0">
        <Table>
          <TableHeader>
            <TableRow>
              <TableHead className="pl-4">GitHub login</TableHead>
              <TableHead>Status</TableHead>
              <TableHead className="text-right">Active tokens</TableHead>
              <TableHead className="pr-4">First sign-in</TableHead>
            </TableRow>
          </TableHeader>
          <TableBody>
            {rows.map((row) => (
              <TableRow key={row.login}>
                <TableCell className="pl-4 font-medium">
                  {row.principal ? (
                    <Link to={`/principals/${row.principal.id}`} className="hover:underline">
                      {row.login}
                    </Link>
                  ) : (
                    row.login
                  )}
                  {row.principal?.id === me.id && (
                    <Badge variant="secondary" className="ml-2">
                      you
                    </Badge>
                  )}
                  {row.principal && row.principal.display_name !== row.principal.name && (
                    <div className="text-xs font-normal text-muted-foreground">{row.principal.display_name}</div>
                  )}
                </TableCell>
                <TableCell>
                  <AdminStatusBadge status={adminStatus(row)} />
                </TableCell>
                <TableCell className="text-right tabular-nums">{row.principal?.active_token_count ?? '—'}</TableCell>
                <TableCell className="pr-4">
                  <Timestamp value={row.principal?.created_at} empty="—" />
                </TableCell>
              </TableRow>
            ))}
          </TableBody>
        </Table>
      </CardContent>
    </Card>
  );
}

function IdentitiesCard({ list }: { list: PrincipalListResponse }) {
  const rows = identities(list);
  return (
    <Card>
      <CardHeader>
        <CardTitle>Identities</CardTitle>
        <CardDescription>Token-only users for CI systems and people. They have no web login.</CardDescription>
      </CardHeader>
      <CardContent className={rows.length ? 'px-0' : undefined}>
        {rows.length === 0 ? (
          <EmptyState
            icon={UsersRound}
            title="No identities yet"
            description="Create an identity, issue it a token, and grant it access to repositories."
          />
        ) : (
          <Table>
            <TableHeader>
              <TableRow>
                <TableHead className="pl-4">Name</TableHead>
                <TableHead>Status</TableHead>
                <TableHead className="text-right">Active tokens</TableHead>
                <TableHead>Created</TableHead>
                <TableHead className="pr-4">Enabled</TableHead>
              </TableRow>
            </TableHeader>
            <TableBody>
              {rows.map((p) => (
                <TableRow key={p.id}>
                  <TableCell className="pl-4 font-medium">
                    <Link to={`/principals/${p.id}`} className="hover:underline">
                      {p.name}
                    </Link>
                    {p.display_name !== p.name && (
                      <div className="text-xs font-normal text-muted-foreground">{p.display_name}</div>
                    )}
                  </TableCell>
                  <TableCell>
                    <PrincipalStatusBadge principal={p} />
                  </TableCell>
                  <TableCell className="text-right tabular-nums">{p.active_token_count}</TableCell>
                  <TableCell>
                    <Timestamp value={p.created_at} />
                  </TableCell>
                  <TableCell className="pr-4">
                    <IdentityEnabledSwitch identity={p} />
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

export function PrincipalsPage() {
  const { me } = useAuth();
  const query = useQuery(listPrincipalsOptions());
  return (
    <>
      <PageHeader
        title="Principals"
        description="Everyone who can authenticate to the registry: GitHub admins and token-only identities."
        actions={
          <>
            <Button variant="outline" asChild>
              <Link to={`/principals/${me.id}?issue=1`}>
                <KeyRound /> Create my CLI token
              </Link>
            </Button>
            <CreateIdentityDialog />
          </>
        }
      />
      {query.isPending ? (
        <LoadingRows rows={6} />
      ) : query.isError ? (
        <QueryError error={query.error} onRetry={() => void query.refetch()} />
      ) : (
        <>
          <AdminsCard list={query.data} />
          <IdentitiesCard list={query.data} />
        </>
      )}
    </>
  );
}
