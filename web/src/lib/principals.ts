import type { PrincipalListResponse, PrincipalSummary } from '@/sdk/types.gen';

export interface AdminRow {
  login: string;
  /** The GitHub principal, once the admin has signed in at least once. */
  principal: PrincipalSummary | undefined;
  /** Listed in MINREGISTRY_ADMIN_GITHUB_LOGINS right now. */
  configured: boolean;
}

export type AdminStatus = 'active' | 'disabled' | 'never-signed-in' | 'removed';

/**
 * Configured admin logins merged with the GitHub principals: admins who never
 * signed in have no principal yet; GitHub principals no longer configured are
 * former admins (they cannot sign in or use their tokens).
 */
export function adminRows(list: PrincipalListResponse): AdminRow[] {
  const github = list.principals.filter((p) => p.kind === 'github');
  const byLogin = new Map(github.map((p) => [p.name.toLowerCase(), p]));
  const configured = new Set(list.admin_logins.map((l) => l.toLowerCase()));
  const rows: AdminRow[] = [...configured].map((login) => ({ login, principal: byLogin.get(login), configured: true }));
  for (const p of github) {
    if (!configured.has(p.name.toLowerCase())) rows.push({ login: p.name, principal: p, configured: false });
  }
  return rows.sort((a, b) => Number(b.configured) - Number(a.configured) || a.login.localeCompare(b.login));
}

export function adminStatus(row: AdminRow): AdminStatus {
  if (!row.configured) return 'removed';
  if (!row.principal) return 'never-signed-in';
  return row.principal.active ? 'active' : 'disabled';
}

export function identities(list: PrincipalListResponse): PrincipalSummary[] {
  return list.principals.filter((p) => p.kind === 'identity').sort((a, b) => a.name.localeCompare(b.name));
}

/** Admins may mint tokens for identities and for their own GitHub principal only. */
export function canIssueToken(principal: PrincipalSummary, me: PrincipalSummary): boolean {
  return principal.kind === 'identity' || principal.id === me.id;
}
