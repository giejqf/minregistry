import { zodResolver } from '@hookform/resolvers/zod';
import { useMutation, useQueryClient } from '@tanstack/react-query';
import { useState } from 'react';
import { Controller, useForm, useWatch } from 'react-hook-form';

import { Button } from '@/components/ui/button';
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog';
import { Field, FieldDescription, FieldError, FieldGroup, FieldLabel } from '@/components/ui/field';
import { Input } from '@/components/ui/input';
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select';
import { Spinner } from '@/components/ui/spinner';
import { invalidate } from '@/lib/invalidate';
import { TOKEN_EXPIRY_OPTIONS, createTokenSchema, tokenExpiresAt, type CreateTokenValues } from '@/lib/schemas';
import { TokenSecretDialog } from '@/pages/principals/token-secret-dialog';
import {
  createTokenMutation,
  getMeQueryKey,
  getPrincipalQueryKey,
  listPrincipalsQueryKey,
  listTokensQueryKey,
} from '@/sdk/@tanstack/react-query.gen';
import type { CreateTokenResponse, PrincipalSummary } from '@/sdk/types.gen';

const DEFAULTS: CreateTokenValues = { name: '', expiry: 'never', customDate: '' };

interface IssueTokenDialogProps {
  principal: PrincipalSummary;
  open: boolean;
  onOpenChange: (open: boolean) => void;
  /** Pre-filled token name, e.g. "laptop". */
  suggestedName?: string;
}

/** Issue-token form; on success the secret is shown once in a TokenSecretDialog. */
export function IssueTokenDialog({ principal, open, onOpenChange, suggestedName = '' }: IssueTokenDialogProps) {
  const queryClient = useQueryClient();
  const [created, setCreated] = useState<CreateTokenResponse | null>(null);
  const form = useForm<CreateTokenValues>({
    resolver: zodResolver(createTokenSchema),
    defaultValues: { ...DEFAULTS, name: suggestedName },
  });
  const expiry = useWatch({ control: form.control, name: 'expiry' });
  const { errors } = form.formState;

  const create = useMutation({
    ...createTokenMutation(),
    onSuccess: async (response) => {
      onOpenChange(false);
      form.reset({ ...DEFAULTS, name: suggestedName });
      setCreated(response);
      await invalidate(
        queryClient,
        getPrincipalQueryKey({ path: { id: principal.id } }),
        listTokensQueryKey({ path: { id: principal.id } }),
        listPrincipalsQueryKey(),
        getMeQueryKey(),
      );
    },
  });

  const onSubmit = form.handleSubmit((values) =>
    create.mutate({
      path: { id: principal.id },
      body: { name: values.name, expires_at: tokenExpiresAt(values) },
    }),
  );

  return (
    <>
      <Dialog
        open={open}
        onOpenChange={(next) => {
          onOpenChange(next);
          if (!next) form.reset({ ...DEFAULTS, name: suggestedName });
        }}
      >
        <DialogContent>
          <form onSubmit={(e) => void onSubmit(e)} noValidate className="grid gap-4">
            <DialogHeader>
              <DialogTitle>Issue token for {principal.name}</DialogTitle>
              <DialogDescription>
                The token is the password for <code className="font-mono">docker login -u {principal.name}</code>. It is
                shown once.
              </DialogDescription>
            </DialogHeader>
            <FieldGroup>
              <Field data-invalid={!!errors.name}>
                <FieldLabel htmlFor="token-name">Name</FieldLabel>
                <Input
                  id="token-name"
                  autoComplete="off"
                  placeholder="github-actions"
                  aria-invalid={!!errors.name}
                  {...form.register('name')}
                />
                <FieldDescription>What the token is for, so you can recognize it later.</FieldDescription>
                <FieldError errors={[errors.name]} />
              </Field>
              <Field>
                <FieldLabel htmlFor="token-expiry">Expires</FieldLabel>
                <Controller
                  control={form.control}
                  name="expiry"
                  render={({ field }) => (
                    <Select value={field.value} onValueChange={field.onChange}>
                      <SelectTrigger id="token-expiry" className="w-full">
                        <SelectValue />
                      </SelectTrigger>
                      <SelectContent>
                        {TOKEN_EXPIRY_OPTIONS.map((o) => (
                          <SelectItem key={o.value} value={o.value}>
                            {o.label}
                          </SelectItem>
                        ))}
                      </SelectContent>
                    </Select>
                  )}
                />
              </Field>
              {expiry === 'custom' && (
                <Field data-invalid={!!errors.customDate}>
                  <FieldLabel htmlFor="token-expiry-date">Expiry date</FieldLabel>
                  <Input
                    id="token-expiry-date"
                    type="date"
                    aria-invalid={!!errors.customDate}
                    {...form.register('customDate')}
                  />
                  <FieldDescription>The token stops working at the end of this day (local time).</FieldDescription>
                  <FieldError errors={[errors.customDate]} />
                </Field>
              )}
            </FieldGroup>
            <DialogFooter>
              <Button type="button" variant="outline" onClick={() => onOpenChange(false)}>
                Cancel
              </Button>
              <Button type="submit" disabled={create.isPending}>
                {create.isPending && <Spinner />}
                Issue token
              </Button>
            </DialogFooter>
          </form>
        </DialogContent>
      </Dialog>
      <TokenSecretDialog created={created} onClose={() => setCreated(null)} />
    </>
  );
}
