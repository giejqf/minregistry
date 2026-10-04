import type { QueryClient, QueryKey } from '@tanstack/react-query';

/**
 * Invalidates every query whose key starts like one of `keys`. The generated
 * query keys are `[{ _id, baseUrl, path?, query? }]` and TanStack matches
 * objects partially, so `listRepositoriesQueryKey()` covers every page/filter.
 */
export function invalidate(queryClient: QueryClient, ...keys: QueryKey[]): Promise<void> {
  return Promise.all(keys.map((queryKey) => queryClient.invalidateQueries({ queryKey }))).then(() => undefined);
}
