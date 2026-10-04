import { CopyButton } from '@/components/copy-button';
import { cn } from '@/lib/utils';

/** A shell command in a code block with a copy button. */
export function CommandSnippet({
  command,
  label = 'command',
  className,
}: {
  command: string;
  label?: string;
  className?: string;
}) {
  return (
    <div className={cn('flex items-start gap-2 rounded-lg border bg-muted/50 p-2 pl-3', className)}>
      <code className="min-w-0 flex-1 font-mono text-xs leading-6 wrap-anywhere whitespace-pre-wrap">{command}</code>
      <CopyButton value={command} label={label} size="icon-sm" />
    </div>
  );
}
