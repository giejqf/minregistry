import { Check, Copy } from 'lucide-react';
import { useEffect, useState } from 'react';
import { toast } from 'sonner';

import { Button } from '@/components/ui/button';
import { copyText } from '@/lib/clipboard';
import { cn } from '@/lib/utils';

interface CopyButtonProps {
  value: string;
  /** What is being copied, for screen readers and the toast ("digest", "command"). */
  label?: string;
  className?: string;
  size?: 'icon-xs' | 'icon-sm' | 'icon';
}

export function CopyButton({ value, label = 'value', className, size = 'icon-xs' }: CopyButtonProps) {
  const [copied, setCopied] = useState(false);

  useEffect(() => {
    if (!copied) return;
    const timer = window.setTimeout(() => setCopied(false), 1500);
    return () => window.clearTimeout(timer);
  }, [copied]);

  return (
    <Button
      type="button"
      variant="ghost"
      size={size}
      className={cn('text-muted-foreground', className)}
      aria-label={`Copy ${label}`}
      title={`Copy ${label}`}
      onClick={() => {
        copyText(value).then(
          () => setCopied(true),
          () => toast.error(`Could not copy the ${label}.`),
        );
      }}
    >
      {copied ? <Check /> : <Copy />}
    </Button>
  );
}
