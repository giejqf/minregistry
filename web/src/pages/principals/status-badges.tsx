import type { ReactNode } from 'react';

import { Badge } from '@/components/ui/badge';
import type { AdminStatus } from '@/lib/principals';
import { cn } from '@/lib/utils';
import type { PermissionLevel, PrincipalSummary, TokenStatus } from '@/sdk/types.gen';

const tone = {
  ok: 'bg-emerald-500/15 text-emerald-700 dark:text-emerald-400',
  warn: 'bg-amber-500/15 text-amber-700 dark:text-amber-400',
  bad: 'bg-destructive/10 text-destructive dark:bg-destructive/20',
  muted: 'bg-muted text-muted-foreground',
} as const;

export function ToneBadge({
  tone: t,
  children,
  className,
}: {
  tone: keyof typeof tone;
  children: ReactNode;
  className?: string;
}) {
  return <Badge className={cn(tone[t], className)}>{children}</Badge>;
}

export function PrincipalStatusBadge({ principal }: { principal: PrincipalSummary }) {
  if (!principal.enabled) return <ToneBadge tone="bad">Disabled</ToneBadge>;
  if (!principal.active) return <ToneBadge tone="muted">Inactive</ToneBadge>;
  return <ToneBadge tone="ok">Active</ToneBadge>;
}

export function AdminStatusBadge({ status }: { status: AdminStatus }) {
  switch (status) {
    case 'active':
      return <ToneBadge tone="ok">Active</ToneBadge>;
    case 'disabled':
      return <ToneBadge tone="bad">Disabled</ToneBadge>;
    case 'never-signed-in':
      return <ToneBadge tone="muted">Never signed in</ToneBadge>;
    case 'removed':
      return <ToneBadge tone="warn">Not in admin list</ToneBadge>;
  }
}

export function TokenStatusBadge({ status }: { status: TokenStatus }) {
  switch (status) {
    case 'active':
      return <ToneBadge tone="ok">Active</ToneBadge>;
    case 'expired':
      return <ToneBadge tone="warn">Expired</ToneBadge>;
    case 'revoked':
      return <ToneBadge tone="muted">Revoked</ToneBadge>;
  }
}

export function LevelBadge({ level }: { level: PermissionLevel }) {
  return (
    <Badge variant={level === 'owner' ? 'default' : level === 'write' ? 'secondary' : 'outline'} className="capitalize">
      {level}
    </Badge>
  );
}

export function KindBadge({ kind }: { kind: PrincipalSummary['kind'] }) {
  return <Badge variant="outline">{kind === 'github' ? 'GitHub admin' : 'Identity'}</Badge>;
}
