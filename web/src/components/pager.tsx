import { ChevronLeft, ChevronRight } from 'lucide-react';

import { Button } from '@/components/ui/button';
import { pageCount, rangeLabel } from '@/lib/pagination';

export function Pager({
  page,
  offset,
  shown,
  total,
  limit,
  onPage,
}: {
  page: number;
  offset: number;
  shown: number;
  total: number;
  limit: number;
  onPage: (page: number) => void;
}) {
  const pages = pageCount(total, limit);
  if (total <= limit && page === 1) return null;
  return (
    <div className="flex items-center justify-between gap-2 text-sm text-muted-foreground">
      <span>{rangeLabel(offset, shown, total)}</span>
      <div className="flex items-center gap-1">
        <Button variant="outline" size="sm" disabled={page <= 1} onClick={() => onPage(page - 1)}>
          <ChevronLeft /> Previous
        </Button>
        <span className="px-2">
          Page {page} of {pages}
        </span>
        <Button variant="outline" size="sm" disabled={page >= pages} onClick={() => onPage(page + 1)}>
          Next <ChevronRight />
        </Button>
      </div>
    </div>
  );
}
