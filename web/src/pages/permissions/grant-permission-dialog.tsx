import { zodResolver } from '@hookform/resolvers/zod';
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { Plus } from 'lucide-react';
import { useState } from 'react';
import { Controller, useForm, useWatch } from 'react-hook-form';
import { toast } from 'sonner';

import { Button } from '@/components/ui/button';
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
  DialogTrigger,
} from '@/components/ui/dialog';
import { Field, FieldDescription, FieldError, FieldGroup, FieldLabel } from '@/components/ui/field';
import {
  Select,
  SelectContent,
  SelectGroup,
  SelectItem,
  SelectLabel,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select';
import { Spinner } from '@/components/ui/spinner';
import { invalidate } from '@/lib/invalidate';
import { LEVEL_DESCRIPTIONS } from '@/lib/permissions';
import { PERMISSION_LEVELS, grantPermissionSchema, type GrantPermissionValues } from '@/lib/schemas';
import {
  getPrincipalQueryKey,
  grantPermissionMutation,
  listPrincipalsOptions,
  listRepositoryPermissionsQueryKey,
} from '@/sdk/@tanstack/react-query.gen';
import type { RepositoryPermissionSummary } from '@/sdk/types.gen';

export function GrantPermissionDialog({
  repositoryId,
  repositoryName,
  existing,
}: {
  repositoryId: string;
  repositoryName: string;
  existing: RepositoryPermissionSummary[];
}) {
  const [open, setOpen] = useState(false);
  const queryClient = useQueryClient();
  const principals = useQuery({ ...listPrincipalsOptions(), enabled: open });
  const form = useForm<GrantPermissionValues>({
    resolver: zodResolver(grantPermissionSchema),
    defaultValues: { principalId: '', level: 'read' },
  });
  const level = useWatch({ control: form.control, name: 'level' });
  const { errors } = form.formState;

  const granted = new Set(existing.map((p) => p.principal.id));
  const candidates = (principals.data?.principals ?? [])
    .filter((p) => !granted.has(p.id))
    .sort((a, b) => a.kind.localeCompare(b.kind) || a.name.localeCompare(b.name));
  const identities = candidates.filter((p) => p.kind === 'identity');
  const admins = candidates.filter((p) => p.kind === 'github');

  const grant = useMutation({
    ...grantPermissionMutation(),
    onSuccess: async (permission) => {
      toast.success(`${permission.principal.name} now has ${permission.level} access to ${repositoryName}.`);
      setOpen(false);
      form.reset();
      await invalidate(
        queryClient,
        listRepositoryPermissionsQueryKey({ path: { id: repositoryId } }),
        getPrincipalQueryKey({ path: { id: permission.principal.id } }),
      );
    },
  });

  const onSubmit = form.handleSubmit((values) =>
    grant.mutate({ path: { id: repositoryId, principal_id: values.principalId }, body: { level: values.level } }),
  );

  return (
    <Dialog
      open={open}
      onOpenChange={(next) => {
        setOpen(next);
        if (!next) form.reset();
      }}
    >
      <DialogTrigger asChild>
        <Button>
          <Plus /> Grant access
        </Button>
      </DialogTrigger>
      <DialogContent>
        <form onSubmit={(e) => void onSubmit(e)} noValidate className="grid gap-4">
          <DialogHeader>
            <DialogTitle>Grant access to {repositoryName}</DialogTitle>
            <DialogDescription>Principals that already have a grant are changed from the table.</DialogDescription>
          </DialogHeader>
          <FieldGroup>
            <Field data-invalid={!!errors.principalId}>
              <FieldLabel htmlFor="grant-principal">Principal</FieldLabel>
              <Controller
                control={form.control}
                name="principalId"
                render={({ field }) => (
                  <Select value={field.value} onValueChange={field.onChange} disabled={principals.isPending}>
                    <SelectTrigger id="grant-principal" className="w-full" aria-invalid={!!errors.principalId}>
                      <SelectValue placeholder={principals.isPending ? 'Loading…' : 'Choose a principal'} />
                    </SelectTrigger>
                    <SelectContent>
                      {identities.length > 0 && (
                        <SelectGroup>
                          <SelectLabel>Identities</SelectLabel>
                          {identities.map((p) => (
                            <SelectItem key={p.id} value={p.id}>
                              {p.name}
                              {!p.enabled && ' (disabled)'}
                            </SelectItem>
                          ))}
                        </SelectGroup>
                      )}
                      {admins.length > 0 && (
                        <SelectGroup>
                          <SelectLabel>GitHub admins</SelectLabel>
                          {admins.map((p) => (
                            <SelectItem key={p.id} value={p.id}>
                              {p.name}
                            </SelectItem>
                          ))}
                        </SelectGroup>
                      )}
                      {candidates.length === 0 && !principals.isPending && (
                        <div className="px-2 py-1.5 text-sm text-muted-foreground">
                          Every principal already has a grant.
                        </div>
                      )}
                    </SelectContent>
                  </Select>
                )}
              />
              {principals.isError && <FieldError>Could not load principals.</FieldError>}
              <FieldError errors={[errors.principalId]} />
            </Field>
            <Field>
              <FieldLabel htmlFor="grant-level">Level</FieldLabel>
              <Controller
                control={form.control}
                name="level"
                render={({ field }) => (
                  <Select value={field.value} onValueChange={field.onChange}>
                    <SelectTrigger id="grant-level" className="w-full">
                      <SelectValue />
                    </SelectTrigger>
                    <SelectContent>
                      {PERMISSION_LEVELS.map((level) => (
                        <SelectItem key={level} value={level} className="capitalize">
                          {level}
                        </SelectItem>
                      ))}
                    </SelectContent>
                  </Select>
                )}
              />
              <FieldDescription>{LEVEL_DESCRIPTIONS[level]}</FieldDescription>
            </Field>
          </FieldGroup>
          <DialogFooter>
            <Button type="button" variant="outline" onClick={() => setOpen(false)}>
              Cancel
            </Button>
            <Button type="submit" disabled={grant.isPending}>
              {grant.isPending && <Spinner />}
              Grant access
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}
