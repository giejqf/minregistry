import { useEffect, useState } from 'react';
import { useSearchParams } from 'react-router';

import { useDebouncedValue } from '@/hooks/use-debounced-value';
import { parsePage } from '@/lib/pagination';

const REPOSITORY_PAGE_SIZE = 50;

/** Search box bound to the `q` search param (debounced); resets to page 1. */
export function useRepositorySearch() {
  const [params, setParams] = useSearchParams();
  const q = params.get('q') ?? '';
  const [input, setInput] = useState(q);
  const debounced = useDebouncedValue(input.trim(), 250);

  useEffect(() => {
    if (debounced === q) return;
    setParams(
      (prev) => {
        const next = new URLSearchParams(prev);
        if (debounced) next.set('q', debounced);
        else next.delete('q');
        next.delete('page');
        return next;
      },
      { replace: true },
    );
  }, [debounced, q, setParams]);

  const { page, offset, limit } = parsePage(params.get('page'), REPOSITORY_PAGE_SIZE);
  const setPage = (n: number) =>
    setParams((prev) => {
      const next = new URLSearchParams(prev);
      if (n <= 1) next.delete('page');
      else next.set('page', String(n));
      return next;
    });

  return { q, input, setInput, page, offset, limit, setPage };
}
