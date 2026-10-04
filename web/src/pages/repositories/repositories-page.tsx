import { keepPreviousData, useQuery } from '@tanstack/react-query';
import { FolderGit2, Search } from 'lucide-react';
import { Link } from 'react-router';

import { PageHeader } from '@/components/page-header';
import { Pager } from '@/components/pager';
import { EmptyState, LoadingRows, QueryError } from '@/components/query-state';
import { RepositorySearchInput } from '@/components/repository-search-input';
import { Timestamp } from '@/components/timestamp';
import { Card, CardContent } from '@/components/ui/card';
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from '@/components/ui/table';
import { useRepositorySearch } from '@/hooks/use-repository-search';
import { formatCount } from '@/lib/format';
import { listRepositoriesOptions } from '@/sdk/@tanstack/react-query.gen';
import type { RepositorySummary } from '@/sdk/types.gen';

function RepositoryTable({ items }: { items: RepositorySummary[] }) {
  return (
    <Table>
      <TableHeader>
        <TableRow>
          <TableHead className="pl-4">Name</TableHead>
          <TableHead className="text-right">Tags</TableHead>
          <TableHead className="text-right">Manifests</TableHead>
          <TableHead>Last push</TableHead>
          <TableHead className="pr-4">Created</TableHead>
        </TableRow>
      </TableHeader>
      <TableBody>
        {items.map((repo) => (
          <TableRow key={repo.id}>
            <TableCell className="pl-4 font-medium">
              <Link to={`/repositories/${repo.id}`} className="hover:underline">
                {repo.name}
              </Link>
            </TableCell>
            <TableCell className="text-right tabular-nums">{formatCount(repo.tag_count)}</TableCell>
            <TableCell className="text-right tabular-nums">{formatCount(repo.manifest_count)}</TableCell>
            <TableCell>
              <Timestamp value={repo.last_push_at} empty="Never" />
            </TableCell>
            <TableCell className="pr-4">
              <Timestamp value={repo.created_at} />
              {repo.created_by && <span className="text-muted-foreground"> by {repo.created_by}</span>}
            </TableCell>
          </TableRow>
        ))}
      </TableBody>
    </Table>
  );
}

export function RepositoriesPage() {
  const search = useRepositorySearch();
  const query = useQuery({
    ...listRepositoriesOptions({
      query: { q: search.q || undefined, limit: search.limit, offset: search.offset },
    }),
    placeholderData: keepPreviousData,
  });

  return (
    <>
      <PageHeader
        title="Repositories"
        description="Repositories are created automatically on the first push; the pusher becomes their owner."
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
            description={`Push an image to create one: docker push ${window.location.host}/<name>:<tag>`}
          />
        )
      ) : (
        <>
          <Card className="py-0">
            <CardContent className="px-0">
              <RepositoryTable items={query.data.items} />
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
