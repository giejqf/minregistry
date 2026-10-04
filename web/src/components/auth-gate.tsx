import { useQuery, useQueryClient } from '@tanstack/react-query';
import { useCallback, useEffect, useMemo, useState, type ReactNode } from 'react';
import { toast } from 'sonner';

import { QueryError } from '@/components/query-state';
import { Spinner } from '@/components/ui/spinner';
import { isUnauthenticated } from '@/lib/api';
import { signOut as endSession } from '@/lib/auth';
import { AuthContext, type AuthState } from '@/lib/auth-context';
import { onSessionExpired } from '@/lib/session-events';
import { SignInPage } from '@/pages/sign-in';
import { getMeOptions } from '@/sdk/@tanstack/react-query.gen';

/**
 * Renders the app for a signed-in admin and the sign-in page otherwise: when
 * `GET /api/v1/me` is 401, after sign-out, or when any API call returns 401.
 */
export function AuthGate({ children }: { children: ReactNode }) {
  const queryClient = useQueryClient();
  const [signedOut, setSignedOut] = useState(false);
  const me = useQuery({ ...getMeOptions(), retry: false, enabled: !signedOut, staleTime: 60_000 });

  const leave = useCallback(() => {
    setSignedOut(true);
    queryClient.clear();
  }, [queryClient]);

  useEffect(() => onSessionExpired(leave), [leave]);

  const signOut = useCallback(async () => {
    try {
      await endSession();
    } catch (error) {
      toast.error(error instanceof Error ? error.message : 'Sign-out failed.');
      return;
    }
    leave();
  }, [leave]);

  const auth = useMemo<AuthState | null>(
    () => (me.data ? { me: me.data.principal, signOut } : null),
    [me.data, signOut],
  );

  if (signedOut || (me.isError && isUnauthenticated(me.error))) return <SignInPage />;
  if (me.isError) {
    return (
      <div className="mx-auto mt-24 max-w-md p-4">
        <QueryError error={me.error} title="Could not reach MinRegistry" onRetry={() => void me.refetch()} />
      </div>
    );
  }
  if (!auth) {
    return (
      <div className="flex min-h-svh items-center justify-center text-muted-foreground">
        <Spinner className="size-6" />
      </div>
    );
  }
  return <AuthContext.Provider value={auth}>{children}</AuthContext.Provider>;
}
