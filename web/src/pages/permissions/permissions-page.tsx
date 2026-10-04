import { keepPreviousData, useQuery } from '@tanstack/react-query';
import { FolderGit2, Search, ShieldCheck } from 'lucide-react';
import { Link } from 'react-router';

import { PageHeader } from '@/components/page-header';
import { Pager } from '@/components/pager';
import { EmptyState, LoadingRows, QueryError } from '@/components/query-state';
import { RepositorySearchInput } from '@/components/repository-search-input';
import { Button } from '@/components/ui/button';
import { Card, CardContent } from '@/components/ui/card';
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from '@/components/ui/table';
import { useRepositorySearch } from '@/hooks/use-repository-search';
import { listRepositoriesOptions } from '@/sdk/@tanstack/react-query.gen';

/** Top-level entry: pick a repository to manage its grants. */
export function PermissionsPage() {
  const search = useRepositorySearch();
  const query = useQuery({
    ...listRepositoriesOptions({ query: { q: search.q || undefined, limit: search.limit, offset: search.offset } }),
    placeholderData: keepPreviousData,
  });

  return (
    <>
      <PageHeader
        title="Permissions"
        description="Access is granted per repository and per principal: read, write or owner. Pick a repository."
      />
      <RepositorySearchInput value={search.input} onChange={search.setInput} />
      {query.isPending ? (
        <LoadingRows rows={5} />
      ) : query.isError ? (
        <QueryError error={query.error} onRetry={() => void query.refetch()} />
      ) : query.data.items.length === 0 ? (
        search.q ? (
          <EmptyState icon={Search} title="No matching repositories" description={`Nothing matches “${search.q}”.`} />
        ) : (
          <EmptyState
            icon={FolderGit2}
            title="No repositories yet"
            description="Permissions are granted per repository."
          />
        )
      ) : (
        <>
          <Card className="py-0">
            <CardContent className="px-0">
              <Table>
                <TableHeader>
                  <TableRow>
                    <TableHead className="pl-4">Repository</TableHead>
                    <TableHead className="w-0 pr-4">
                      <span className="sr-only">Actions</span>
                    </TableHead>
                  </TableRow>
                </TableHeader>
                <TableBody>
                  {query.data.items.map((repo) => (
                    <TableRow key={repo.id}>
                      <TableCell className="pl-4 font-medium">
                        <Link to={`/repositories/${repo.id}/permissions`} className="hover:underline">
                          {repo.name}
                        </Link>
                      </TableCell>
                      <TableCell className="pr-4">
                        <Button variant="outline" size="sm" asChild>
                          <Link to={`/repositories/${repo.id}/permissions`}>
                            <ShieldCheck /> Manage permissions
                          </Link>
                        </Button>
                      </TableCell>
                    </TableRow>
                  ))}
                </TableBody>
              </Table>
            </CardContent>
          </Card>
          <Pager
            page={search.page}
            offset={search.offset}
            shown={query.data.items.length}
            total={query.data.total}
            limit={search.limit}
            onPage={search.setPage}
          />
        </>
      )}
    </>
  );
}
