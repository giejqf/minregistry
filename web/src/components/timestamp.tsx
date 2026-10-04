import { formatDateTime, formatRelative } from '@/lib/format';

/** Relative time ("5 minutes ago"), with the local time and the full RFC 3339 value on hover. */
export function Timestamp({ value, empty = '—' }: { value: string | null | undefined; empty?: string }) {
  if (!value) return <span className="text-muted-foreground">{empty}</span>;
  return (
    <time dateTime={value} title={`${formatDateTime(value)}\n${value}`} className="whitespace-nowrap">
      {formatRelative(value)}
    </time>
  );
}
