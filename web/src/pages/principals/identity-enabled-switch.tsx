import { useMutation, useQueryClient } from '@tanstack/react-query';
import { useState } from 'react';
import { toast } from 'sonner';

import {
  AlertDialog,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
} from '@/components/ui/alert-dialog';
import { Button } from '@/components/ui/button';
import { Spinner } from '@/components/ui/spinner';
import { Switch } from '@/components/ui/switch';
import { invalidate } from '@/lib/invalidate';
import { getPrincipalQueryKey, listPrincipalsQueryKey, updatePrincipalMutation } from '@/sdk/@tanstack/react-query.gen';
import type { PrincipalSummary } from '@/sdk/types.gen';

/** Enables an identity at once; disabling (which blocks all its tokens) asks first. */
export function IdentityEnabledSwitch({ identity }: { identity: PrincipalSummary }) {
  const queryClient = useQueryClient();
  const [confirming, setConfirming] = useState(false);
  const update = useMutation({
    ...updatePrincipalMutation(),
    onSuccess: async (updated) => {
      toast.success(`${updated.name} ${updated.enabled ? 'enabled' : 'disabled'}.`);
      setConfirming(false);
      await invalidate(queryClient, listPrincipalsQueryKey(), getPrincipalQueryKey({ path: { id: identity.id } }));
    },
  });
  const setEnabled = (enabled: boolean) => update.mutate({ path: { id: identity.id }, body: { enabled } });

  return (
    <>
      <Switch
        checked={identity.enabled}
        disabled={update.isPending}
        aria-label={`${identity.enabled ? 'Disable' : 'Enable'} ${identity.name}`}
        onCheckedChange={(checked) => (checked ? setEnabled(true) : setConfirming(true))}
      />
      <AlertDialog open={confirming} onOpenChange={(open) => !update.isPending && setConfirming(open)}>
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>Disable {identity.name}?</AlertDialogTitle>
            <AlertDialogDescription>
              All of its tokens stop working immediately ({identity.active_token_count} active). Its tokens and
              permissions are kept; enable it again to restore access.
            </AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel disabled={update.isPending}>Cancel</AlertDialogCancel>
            <Button variant="destructive" disabled={update.isPending} onClick={() => setEnabled(false)}>
              {update.isPending && <Spinner />}
              Disable identity
            </Button>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </>
  );
}
