import { zodResolver } from '@hookform/resolvers/zod';
import { keepPreviousData, useQuery } from '@tanstack/react-query';
import { ChevronDown, ChevronLeft, ChevronRight, ScrollText } from 'lucide-react';
import { Fragment, useMemo, useState } from 'react';
import { Controller, useForm } from 'react-hook-form';
import { useSearchParams } from 'react-router';

import { CopyButton } from '@/components/copy-button';
import { PageHeader } from '@/components/page-header';
import { EmptyState, LoadingRows, QueryError } from '@/components/query-state';
import { Timestamp } from '@/components/timestamp';
import { Badge } from '@/components/ui/badge';
import { Button } from '@/components/ui/button';
import { Card, CardContent } from '@/components/ui/card';
import { Field, FieldError, FieldLabel } from '@/components/ui/field';
import { Input } from '@/components/ui/input';
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select';
import { Spinner } from '@/components/ui/spinner';
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from '@/components/ui/table';
import {
  AUDIT_OUTCOMES,
  EMPTY_AUDIT_FILTERS,
  auditQuery,
  hasActiveFilters,
  hasNewerPage,
  newerPage,
  olderPage,
  parseAuditSearch,
  withFilters,
  type AuditFilters,
} from '@/lib/audit-search';
import { formatDateTime, fromLocalInputValue, shortDigest, toLocalInputValue } from '@/lib/format';
import { auditFiltersSchema, type AuditFilterValues } from '@/lib/schemas';
import { cn } from '@/lib/utils';
import {
  listAuditActionsOptions,
  listAuditEventsOptions,
  listPrincipalsOptions,
  listRepositoriesOptions,
} from '@/sdk/@tanstack/react-query.gen';
import type { AuditEventSummary, AuditOutcome } from '@/sdk/types.gen';

const ANY = '__any__';

const OUTCOME_TONE: Record<AuditOutcome, string> = {
  ok: 'bg-emerald-500/15 text-emerald-700 dark:text-emerald-400',
  denied: 'bg-amber-500/15 text-amber-700 dark:text-amber-400',
  error: 'bg-destructive/10 text-destructive dark:bg-destructive/20',
};

export function OutcomeBadge({ outcome }: { outcome: AuditOutcome }) {
  return <Badge className={OUTCOME_TONE[outcome]}>{outcome}</Badge>;
}

function toFormValues(filters: AuditFilters): AuditFilterValues {
  return { ...filters, from: toLocalInputValue(filters.from), to: toLocalInputValue(filters.to) };
}

function FiltersForm({ filters, onApply }: { filters: AuditFilters; onApply: (filters: AuditFilters) => void }) {
  const actions = useQuery(listAuditActionsOptions());
  const principals = useQuery(listPrincipalsOptions());
  const repositories = useQuery(listRepositoriesOptions({ query: { limit: 500 } }));
  const form = useForm<AuditFilterValues>({
    resolver: zodResolver(auditFiltersSchema),
    defaultValues: toFormValues(filters),
  });
  const { errors } = form.formState;

  const onSubmit = form.handleSubmit((values) =>
    onApply({ ...values, from: fromLocalInputValue(values.from), to: fromLocalInputValue(values.to) }),
  );

  return (
    <form
      onSubmit={(e) => void onSubmit(e)}
      noValidate
      className="grid gap-3 sm:grid-cols-2 lg:grid-cols-3"
      aria-label="Audit filters"
    >
      <Field>
        <FieldLabel htmlFor="audit-principal">Principal</FieldLabel>
        <Input
          id="audit-principal"
          list="audit-principals"
          placeholder="Any"
          autoComplete="off"
          {...form.register('principal')}
        />
        <datalist id="audit-principals">
          {principals.data?.principals.map((p) => (
            <option key={p.id} value={p.name} />
          ))}
        </datalist>
      </Field>
      <Field>
        <FieldLabel htmlFor="audit-repository">Repository</FieldLabel>
        <Input
          id="audit-repository"
          list="audit-repositories"
          placeholder="Any"
          autoComplete="off"
          {...form.register('repository')}
        />
        <datalist id="audit-repositories">
          {repositories.data?.items.map((r) => (
            <option key={r.id} value={r.name} />
          ))}
        </datalist>
      </Field>
      <Field>
        <FieldLabel htmlFor="audit-action">Action</FieldLabel>
        <Controller
          control={form.control}
          name="action"
          render={({ field }) => (
            <Select value={field.value || ANY} onValueChange={(v) => field.onChange(v === ANY ? '' : v)}>
              <SelectTrigger id="audit-action" className="w-full">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                <SelectItem value={ANY}>Any action</SelectItem>
                {(actions.data?.actions ?? []).map((a) => (
                  <SelectItem key={a} value={a}>
                    {a}
                  </SelectItem>
                ))}
                {field.value && !actions.data?.actions.includes(field.value) && (
                  <SelectItem value={field.value}>{field.value}</SelectItem>
                )}
              </SelectContent>
            </Select>
          )}
        />
      </Field>
      <Field>
        <FieldLabel htmlFor="audit-outcome">Outcome</FieldLabel>
        <Controller
          control={form.control}
          name="outcome"
          render={({ field }) => (
            <Select value={field.value || ANY} onValueChange={(v) => field.onChange(v === ANY ? '' : v)}>
              <SelectTrigger id="audit-outcome" className="w-full">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                <SelectItem value={ANY}>Any outcome</SelectItem>
                {AUDIT_OUTCOMES.map((o) => (
                  <SelectItem key={o} value={o}>
                    {o}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          )}
        />
      </Field>
      <Field>
        <FieldLabel htmlFor="audit-from">From</FieldLabel>
        <Input id="audit-from" type="datetime-local" {...form.register('from')} />
      </Field>
      <Field data-invalid={!!errors.to}>
        <FieldLabel htmlFor="audit-to">To</FieldLabel>
        <Input id="audit-to" type="datetime-local" aria-invalid={!!errors.to} {...form.register('to')} />
        <FieldError errors={[errors.to]} />
      </Field>
      <div className="flex items-center gap-2 sm:col-span-2 lg:col-span-3">
        <Button type="submit">Apply filters</Button>
        <Button
          type="button"
          variant="ghost"
          disabled={!hasActiveFilters(filters)}
          onClick={() => onApply(EMPTY_AUDIT_FILTERS)}
        >
          Clear
        </Button>
      </div>
    </form>
  );
}

function DetailRow({ event }: { event: AuditEventSummary }) {
  const json = JSON.stringify(event.detail, null, 2);
  const facts: Array<[string, string | null | undefined]> = [
    ['Time', `${formatDateTime(event.ts)} (${event.ts})`],
    ['Event id', event.id],
    ['Principal id', event.principal_id],
    ['Reference', event.reference],
    ['Digest', event.digest],
    ['Client IP', event.client_ip],
    ['User agent', event.user_agent],
  ];
  return (
    <div className="grid gap-4 py-2 lg:grid-cols-2">
      <dl className="grid grid-cols-[max-content_1fr] gap-x-4 gap-y-1 text-sm">
        {facts.map(([label, value]) => (
          <Fragment key={label}>
            <dt className="text-muted-foreground">{label}</dt>
            <dd className="font-mono text-xs leading-5 break-all">{value || '—'}</dd>
          </Fragment>
        ))}
      </dl>
      <div className="relative">
        <pre className="max-h-80 overflow-auto rounded-lg border bg-muted/50 p-3 font-mono text-xs leading-5">
          {json}
        </pre>
        <CopyButton value={json} label="detail JSON" className="absolute top-1 right-1" />
      </div>
    </div>
  );
}

function EventRows({ event }: { event: AuditEventSummary }) {
  const [open, setOpen] = useState(false);
  const reference = event.reference ?? event.digest;
  return (
    <>
      <TableRow className={cn('cursor-pointer', open && 'border-b-0 bg-muted/30')} onClick={() => setOpen((v) => !v)}>
        <TableCell className="pl-2">
          <Button
            variant="ghost"
            size="icon-xs"
            aria-expanded={open}
            aria-label={open ? 'Hide details' : 'Show details'}
            onClick={(e) => {
              e.stopPropagation();
              setOpen((v) => !v);
            }}
          >
            <ChevronDown className={cn('transition-transform', !open && '-rotate-90')} />
          </Button>
        </TableCell>
        <TableCell>
          <Timestamp value={event.ts} />
        </TableCell>
        <TableCell className="font-medium">
          {event.principal_name ?? <span className="text-muted-foreground">anonymous</span>}
        </TableCell>
        <TableCell>
          <code className="font-mono text-xs">{event.action}</code>
        </TableCell>
        <TableCell className="max-w-56 truncate" title={event.repository ?? undefined}>
          {event.repository ?? <span className="text-muted-foreground">—</span>}
        </TableCell>
        <TableCell className="max-w-48 truncate font-mono text-xs" title={reference ?? undefined}>
          {reference ? (
            reference.startsWith('sha256:') ? (
              shortDigest(reference)
            ) : (
              reference
            )
          ) : (
            <span className="text-muted-foreground">—</span>
          )}
        </TableCell>
        <TableCell className="pr-4">
          <OutcomeBadge outcome={event.outcome} />
        </TableCell>
      </TableRow>
      {open && (
        <TableRow className="bg-muted/30 hover:bg-muted/30">
          <TableCell colSpan={7} className="px-4 whitespace-normal">
            <DetailRow event={event} />
          </TableCell>
        </TableRow>
      )}
    </>
  );
}

export function AuditPage() {
  const [params, setParams] = useSearchParams();
  const page = useMemo(() => parseAuditSearch(params), [params]);
  // Remount the form when the URL's filters change (back/forward, "Clear").
  const filtersKey = withFilters(page.filters).toString();
  const query = useQuery({
    ...listAuditEventsOptions({ query: auditQuery(page) }),
    placeholderData: keepPreviousData,
  });
  const nextCursor = query.data?.next_cursor;

  return (
    <>
      <PageHeader title="Audit log" description="Who did what to which repository, newest first." />
      <Card>
        <CardContent>
          <FiltersForm key={filtersKey} filters={page.filters} onApply={(filters) => setParams(withFilters(filters))} />
        </CardContent>
      </Card>
      {query.isPending ? (
        <LoadingRows rows={8} />
      ) : query.isError ? (
        <QueryError error={query.error} onRetry={() => void query.refetch()} />
      ) : query.data.items.length === 0 ? (
        <EmptyState
          icon={ScrollText}
          title="No events"
          description={
            hasActiveFilters(page.filters) ? 'No audit events match these filters.' : 'Nothing has been recorded yet.'
          }
        />
      ) : (
        <Card className="py-0">
          <CardContent className="px-0">
            <Table>
              <TableHeader>
                <TableRow>
                  <TableHead className="w-0 pl-2">
                    <span className="sr-only">Details</span>
                  </TableHead>
                  <TableHead>Time</TableHead>
                  <TableHead>Principal</TableHead>
                  <TableHead>Action</TableHead>
                  <TableHead>Repository</TableHead>
                  <TableHead>Reference</TableHead>
                  <TableHead className="pr-4">Outcome</TableHead>
                </TableRow>
              </TableHeader>
              <TableBody>
                {query.data.items.map((event) => (
                  <EventRows key={event.id} event={event} />
                ))}
              </TableBody>
            </Table>
          </CardContent>
        </Card>
      )}
      <div className="flex items-center justify-end gap-2">
        {query.isFetching && !query.isPending && <Spinner className="text-muted-foreground" />}
        <Button variant="outline" size="sm" disabled={!hasNewerPage(page)} onClick={() => setParams(newerPage(page))}>
          <ChevronLeft /> Newer
        </Button>
        <Button
          variant="outline"
          size="sm"
          disabled={!nextCursor}
          onClick={() => nextCursor && setParams(olderPage(page, nextCursor))}
        >
          Older <ChevronRight />
        </Button>
      </div>
    </>
  );
}
