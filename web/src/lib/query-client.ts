import { MutationCache, QueryCache, QueryClient } from '@tanstack/react-query';
import { toast } from 'sonner';

import { ApiError, errorMessage, isUnauthenticated } from '@/lib/api';
import { notifySessionExpired } from '@/lib/session-events';

declare module '@tanstack/react-query' {
  interface Register {
    mutationMeta: {
      /** The mutation reports its own errors; skip the global error toast. */
      silent?: boolean;
    };
  }
}

/** Client errors (4xx) are final; retry network and server errors twice. */
export function shouldRetry(failureCount: number, error: unknown): boolean {
  if (error instanceof ApiError && error.status >= 400 && error.status < 500) return false;
  return failureCount < 2;
}

export function createQueryClient(): QueryClient {
  return new QueryClient({
    queryCache: new QueryCache({
      onError: (error) => {
        if (isUnauthenticated(error)) notifySessionExpired();
      },
    }),
    mutationCache: new MutationCache({
      onError: (error, _variables, _context, mutation) => {
        if (isUnauthenticated(error)) {
          notifySessionExpired();
          return;
        }
        if (!mutation.meta?.silent) toast.error(errorMessage(error));
      },
    }),
    defaultOptions: {
      queries: {
        retry: shouldRetry,
        staleTime: 10_000,
      },
      mutations: {
        retry: false,
      },
    },
  });
}
