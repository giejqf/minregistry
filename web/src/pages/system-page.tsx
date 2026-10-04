import { zodResolver } from '@hookform/resolvers/zod';
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { Recycle, Upload } from 'lucide-react';
import { useState, type ReactNode } from 'react';
import { Controller, useForm, useWatch } from 'react-hook-form';
import { toast } from 'sonner';

import { PageHeader } from '@/components/page-header';
import { EmptyState, LoadingRows, QueryError } from '@/components/query-state';
import { Timestamp } from '@/components/timestamp';
import {
  AlertDialog,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
} from '@/components/ui/alert-dialog';
import { Badge } from '@/components/ui/badge';
import { Button } from '@/components/ui/button';
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card';
import { Checkbox } from '@/components/ui/checkbox';
import { Field, FieldContent, FieldDescription, FieldError, FieldGroup, FieldLabel } from '@/components/ui/field';
import { Input } from '@/components/ui/input';
import { Spinner } from '@/components/ui/spinner';
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from '@/components/ui/table';
import { invalidate } from '@/lib/invalidate';
import { formatBytes, formatCount, formatDuration, formatMillis } from '@/lib/format';
import { gcSchema, type GcValues } from '@/lib/schemas';
import {
  getSystemOptions,
  getSystemQueryKey,
  listRepositoriesQueryKey,
  listUploadsOptions,
  runGcMutation,
} from '@/sdk/@tanstack/react-query.gen';
import type { GcResponse, SystemResponse } from '@/sdk/types.gen';

function Facts({ items }: { items: Array<[string, ReactNode]> }) {
  return (
    <dl className="grid grid-cols-[max-content_1fr] gap-x-6 gap-y-2 text-sm">
      {items.map(([label, value]) => (
        <div key={label} className="contents">
          <dt className="text-muted-foreground">{label}</dt>
          <dd className="min-w-0 break-all">{value}</dd>
        </div>
      ))}
    </dl>
  );
}

function InfoCard({
  title,
  description,
  items,
}: {
  title: string;
  description?: string;
  items: Array<[string, ReactNode]>;
}) {
  return (
    <Card>
      <CardHeader>
        <CardTitle>{title}</CardTitle>
        {description && <CardDescription>{description}</CardDescription>}
      </CardHeader>
      <CardContent>
        <Facts items={items} />
      </CardContent>
    </Card>
  );
}

const mono = (v: string) => <code className="font-mono text-xs">{v}</code>;

function SystemInfo({ system }: { system: SystemResponse }) {
  return (
    <div className="grid gap-4 md:grid-cols-2">
      <InfoCard
        title="Server"
        items={[
          ['Version', system.version],
          ['Started', <Timestamp key="s" value={system.started_at} />],
        ]}
      />
      <InfoCard
        title="Totals"
        items={[
          ['Repositories', formatCount(system.repository_count)],
          ['Manifests', formatCount(system.manifest_count)],
          ['Blobs', formatCount(system.blob_count)],
          ['Blob storage', formatBytes(system.blob_bytes)],
        ]}
      />
      <InfoCard
        title="Storage"
        items={[
          [
            'Backend',
            <Badge key="b" variant="secondary" className="uppercase">
              {system.storage.backend}
            </Badge>,
          ],
          ['Location', mono(system.storage.location)],
          ['Database', mono(system.database.path)],
          ['Database size', formatBytes(system.database.size_bytes)],
        ]}
      />
      <InfoCard
        title="Uploads"
        items={[
          ['Staging directory', mono(system.upload_dir)],
          ['Session TTL', formatDuration(system.upload_ttl_seconds)],
          ['In flight', formatCount(system.uploads_in_flight)],
        ]}
      />
      <InfoCard
        title="Garbage collection"
        items={[
          ['Schedule', system.gc_cron ? mono(system.gc_cron) : 'Manual only'],
          ['Minimum age', formatDuration(system.gc_min_age_seconds)],
        ]}
      />
      <InfoCard
        title="Audit"
        items={[
          ['Retention', system.audit_retention_days === 0 ? 'Forever' : `${system.audit_retention_days} days`],
          ['Blob downloads audited', system.audit_blob_reads ? 'Yes' : 'No'],
        ]}
      />
    </div>
  );
}

function UploadsCard() {
  const query = useQuery(listUploadsOptions());
  return (
    <Card>
      <CardHeader>
        <CardTitle>Upload sessions in flight</CardTitle>
        <CardDescription>
          Blob uploads that have started but not completed. Expired sessions are cleaned up automatically.
        </CardDescription>
      </CardHeader>
      <CardContent className={query.data?.length ? 'px-0' : undefined}>
        {query.isPending ? (
          <LoadingRows rows={2} />
        ) : query.isError ? (
          <QueryError error={query.error} onRetry={() => void query.refetch()} />
        ) : query.data.length === 0 ? (
          <EmptyState icon={Upload} title="No uploads in flight" />
        ) : (
          <Table>
            <TableHeader>
              <TableRow>
                <TableHead className="pl-4">Session</TableHead>
                <TableHead>Repository</TableHead>
                <TableHead>Principal</TableHead>
                <TableHead className="text-right">Received</TableHead>
                <TableHead>Started</TableHead>
                <TableHead className="pr-4">Last activity</TableHead>
              </TableRow>
            </TableHeader>
            <TableBody>
              {query.data.map((u) => (
                <TableRow key={u.uuid}>
                  <TableCell className="pl-4 font-mono text-xs" title={u.uuid}>
                    {u.uuid.slice(0, 8)}…
                  </TableCell>
                  <TableCell>{u.repository}</TableCell>
                  <TableCell>{u.principal}</TableCell>
                  <TableCell className="text-right tabular-nums">{formatBytes(u.offset)}</TableCell>
                  <TableCell>
                    <Timestamp value={u.started_at} />
                  </TableCell>
                  <TableCell className="pr-4">
                    <Timestamp value={u.last_activity_at} />
                  </TableCell>
                </TableRow>
              ))}
            </TableBody>
          </Table>
        )}
      </CardContent>
    </Card>
  );
}

export function GcReport({ report }: { report: GcResponse }) {
  return (
    <div className="space-y-3 rounded-lg border p-4" aria-label="Garbage collection report">
      <div className="flex flex-wrap items-center gap-2">
        {report.dry_run ? <Badge variant="secondary">Dry run — nothing was deleted</Badge> : <Badge>Completed</Badge>}
        {report.errors > 0 && <Badge variant="destructive">{report.errors} errors</Badge>}
        <span className="text-xs text-muted-foreground">
          {report.delete_untagged ? 'including untagged manifests' : 'tagged content kept'} · min age{' '}
          {formatDuration(report.min_age_seconds)} · took {formatMillis(report.duration_ms)}
        </span>
      </div>
      <Facts
        items={[
          [report.dry_run ? 'Manifests to delete' : 'Manifests deleted', formatCount(report.manifests_deleted)],
          [report.dry_run ? 'Blobs to delete' : 'Blobs deleted', formatCount(report.blobs_deleted)],
          [report.dry_run ? 'Space to free' : 'Space freed', formatBytes(report.bytes_freed)],
          [
            report.dry_run ? 'Orphaned objects to delete' : 'Orphaned objects deleted',
            formatCount(report.orphans_deleted),
          ],
          ['Blobs kept (younger than min age)', formatCount(report.blobs_kept_young)],
          ['Errors', formatCount(report.errors)],
        ]}
      />
    </div>
  );
}

function sameOptions(report: GcResponse | null, values: GcValues): boolean {
  return (
    report !== null &&
    report.delete_untagged === values.deleteUntagged &&
    report.min_age_seconds === values.minAgeSeconds
  );
}

function GcCard({ system }: { system: SystemResponse }) {
  const queryClient = useQueryClient();
  const [dryRun, setDryRun] = useState<GcResponse | null>(null);
  const [lastRun, setLastRun] = useState<GcResponse | null>(null);
  const [confirming, setConfirming] = useState(false);
  const form = useForm<GcValues>({
    resolver: zodResolver(gcSchema),
    defaultValues: { deleteUntagged: false, minAgeSeconds: system.gc_min_age_seconds },
    mode: 'onChange',
  });
  const values = useWatch({ control: form.control }) as GcValues;
  const { errors } = form.formState;

  const gc = useMutation({
    ...runGcMutation(),
    onSuccess: async (report) => {
      if (report.dry_run) {
        setDryRun(report);
        setLastRun(null);
        return;
      }
      setLastRun(report);
      setDryRun(null);
      setConfirming(false);
      toast.success(`Garbage collection finished: ${formatBytes(report.bytes_freed)} freed.`);
      await invalidate(queryClient, getSystemQueryKey(), listRepositoriesQueryKey());
    },
  });

  const run = (dry: boolean) =>
    form.handleSubmit((v) =>
      gc.mutate({ body: { dry_run: dry, delete_untagged: v.deleteUntagged, min_age_seconds: v.minAgeSeconds } }),
    )();

  const dryRunMatches = sameOptions(dryRun, values);
  const minAge = Number.isFinite(values.minAgeSeconds) ? formatDuration(values.minAgeSeconds) : '—';

  return (
    <Card>
      <CardHeader>
        <CardTitle>Run garbage collection</CardTitle>
        <CardDescription>
          Deletes blobs no tag or manifest references. Always start with a dry run to see what would be deleted.
        </CardDescription>
      </CardHeader>
      <CardContent className="space-y-4">
        <FieldGroup className="max-w-lg">
          <Field orientation="horizontal">
            <Controller
              control={form.control}
              name="deleteUntagged"
              render={({ field }) => (
                <Checkbox id="gc-untagged" checked={field.value} onCheckedChange={(v) => field.onChange(v === true)} />
              )}
            />
            <FieldContent>
              <FieldLabel htmlFor="gc-untagged">Delete untagged manifests</FieldLabel>
              <FieldDescription>
                Also remove manifests no tag reaches (directly, via an index or as a referrer).
              </FieldDescription>
            </FieldContent>
          </Field>
          <Field data-invalid={!!errors.minAgeSeconds}>
            <FieldLabel htmlFor="gc-min-age">Minimum age (seconds)</FieldLabel>
            <Input
              id="gc-min-age"
              type="number"
              min={0}
              step={1}
              className="max-w-40"
              aria-invalid={!!errors.minAgeSeconds}
              {...form.register('minAgeSeconds', { valueAsNumber: true })}
            />
            <FieldDescription>
              Content younger than this is kept ({minAge}; server default {formatDuration(system.gc_min_age_seconds)}).
            </FieldDescription>
            <FieldError errors={[errors.minAgeSeconds]} />
          </Field>
        </FieldGroup>
        <div className="flex flex-wrap items-center gap-2">
          <Button variant="outline" disabled={gc.isPending} onClick={() => void run(true)}>
            {gc.isPending && !confirming && <Spinner />}
            Dry run
          </Button>
          <Button variant="destructive" disabled={!dryRunMatches || gc.isPending} onClick={() => setConfirming(true)}>
            <Recycle /> Run garbage collection
          </Button>
          {!dryRunMatches && (
            <span className="text-xs text-muted-foreground">Run a dry run with these options first.</span>
          )}
        </div>
        {dryRun && <GcReport report={dryRun} />}
        {lastRun && <GcReport report={lastRun} />}
      </CardContent>
      <AlertDialog open={confirming} onOpenChange={(open) => !gc.isPending && setConfirming(open)}>
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>Run garbage collection?</AlertDialogTitle>
            <AlertDialogDescription asChild>
              <div className="space-y-2">
                {dryRun && (
                  <p>
                    The dry run found {formatCount(dryRun.manifests_deleted)} manifests and{' '}
                    {formatCount(dryRun.blobs_deleted)} blobs ({formatBytes(dryRun.bytes_freed)}) to delete.
                  </p>
                )}
                <p>Deleted content cannot be recovered. Images still referenced by a tag are not affected.</p>
              </div>
            </AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel disabled={gc.isPending}>Cancel</AlertDialogCancel>
            <Button variant="destructive" disabled={gc.isPending} onClick={() => void run(false)}>
              {gc.isPending && <Spinner />}
              Run garbage collection
            </Button>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </Card>
  );
}

export function SystemPage() {
  const query = useQuery(getSystemOptions());
  return (
    <>
      <PageHeader title="System" description="Storage, database, uploads and garbage collection." />
      {query.isPending ? (
        <LoadingRows rows={8} />
      ) : query.isError ? (
        <QueryError error={query.error} onRetry={() => void query.refetch()} />
      ) : (
        <>
          <SystemInfo system={query.data} />
          <UploadsCard />
          <GcCard system={query.data} />
        </>
      )}
    </>
  );
}
