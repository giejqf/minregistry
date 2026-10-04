import { CopyButton } from '@/components/copy-button';
import { shortDigest } from '@/lib/format';
import { cn } from '@/lib/utils';

export function Digest({ digest, className, copy = true }: { digest: string; className?: string; copy?: boolean }) {
  return (
    <span className={cn('inline-flex items-center gap-0.5 whitespace-nowrap', className)}>
      <code className="font-mono text-xs" title={digest}>
        {shortDigest(digest)}
      </code>
      {copy && <CopyButton value={digest} label="digest" />}
    </span>
  );
}
