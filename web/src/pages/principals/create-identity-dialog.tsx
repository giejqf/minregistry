import { zodResolver } from '@hookform/resolvers/zod';
import { useMutation, useQueryClient } from '@tanstack/react-query';
import { Plus } from 'lucide-react';
import { useState } from 'react';
import { useForm } from 'react-hook-form';
import { useNavigate } from 'react-router';
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
import { Input } from '@/components/ui/input';
import { Spinner } from '@/components/ui/spinner';
import { invalidate } from '@/lib/invalidate';
import { createIdentitySchema, type CreateIdentityValues } from '@/lib/schemas';
import { createIdentityMutation, listPrincipalsQueryKey } from '@/sdk/@tanstack/react-query.gen';

export function CreateIdentityDialog() {
  const [open, setOpen] = useState(false);
  const queryClient = useQueryClient();
  const navigate = useNavigate();
  const form = useForm<CreateIdentityValues>({
    resolver: zodResolver(createIdentitySchema),
    defaultValues: { name: '', display_name: '' },
  });
  const create = useMutation({
    ...createIdentityMutation(),
    onSuccess: async (principal) => {
      toast.success(`Identity ${principal.name} created. Issue a token so it can sign in with docker login.`);
      setOpen(false);
      form.reset();
      await invalidate(queryClient, listPrincipalsQueryKey());
      await navigate(`/principals/${principal.id}`);
    },
  });

  const onSubmit = form.handleSubmit((values) =>
    create.mutate({ body: { name: values.name, display_name: values.display_name || null } }),
  );
  const { errors } = form.formState;

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
          <Plus /> Create identity
        </Button>
      </DialogTrigger>
      <DialogContent>
        <form onSubmit={(e) => void onSubmit(e)} noValidate className="grid gap-4">
          <DialogHeader>
            <DialogTitle>Create identity</DialogTitle>
            <DialogDescription>
              Identities are token-only users for CI systems and people. They cannot sign in to this UI.
            </DialogDescription>
          </DialogHeader>
          <FieldGroup>
            <Field data-invalid={!!errors.name}>
              <FieldLabel htmlFor="identity-name">Name</FieldLabel>
              <Input
                id="identity-name"
                autoComplete="off"
                placeholder="ci-deploy"
                aria-invalid={!!errors.name}
                {...form.register('name')}
              />
              <FieldDescription>
                Used as the docker login username. Lower-case letters, digits, “.”, “_” and “-”.
              </FieldDescription>
              <FieldError errors={[errors.name]} />
            </Field>
            <Field data-invalid={!!errors.display_name}>
              <FieldLabel htmlFor="identity-display-name">Display name (optional)</FieldLabel>
              <Input
                id="identity-display-name"
                autoComplete="off"
                placeholder="CI deploy pipeline"
                aria-invalid={!!errors.display_name}
                {...form.register('display_name')}
              />
              <FieldError errors={[errors.display_name]} />
            </Field>
          </FieldGroup>
          <DialogFooter>
            <Button type="button" variant="outline" onClick={() => setOpen(false)}>
              Cancel
            </Button>
            <Button type="submit" disabled={create.isPending}>
              {create.isPending && <Spinner />}
              Create identity
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}
