import { describe, expect, it } from 'vitest';

import { adminRows, adminStatus, canIssueToken, identities } from '@/lib/principals';
import type { PrincipalSummary } from '@/sdk/types.gen';

function principal(id: string, name: string, overrides: Partial<PrincipalSummary> = {}): PrincipalSummary {
  return {
    id,
    kind: 'github',
    name,
    display_name: name,
    enabled: true,
    is_admin: true,
    active: true,
    active_token_count: 0,
    created_at: '2026-01-01T00:00:00Z',
    ...overrides,
  };
}

describe('adminRows', () => {
  const list = {
    admin_logins: ['alice', 'Bob'],
    principals: [
      principal('1', 'alice'),
      principal('2', 'carol', { is_admin: false, active: false }),
      principal('3', 'ci', { kind: 'identity', is_admin: false }),
    ],
  };

  it('merges configured logins with GitHub principals', () => {
    const rows = adminRows(list);
    expect(rows.map((r) => [r.login, adminStatus(r)])).toEqual([
      ['alice', 'active'],
      ['bob', 'never-signed-in'],
      ['carol', 'removed'],
    ]);
  });

  it('lists identities only once, sorted', () => {
    expect(identities(list).map((p) => p.name)).toEqual(['ci']);
  });
});

describe('canIssueToken', () => {
  const me = principal('1', 'alice');
  it('allows identities and the own GitHub principal only', () => {
    expect(canIssueToken(principal('3', 'ci', { kind: 'identity' }), me)).toBe(true);
    expect(canIssueToken(me, me)).toBe(true);
    expect(canIssueToken(principal('2', 'bob'), me)).toBe(false);
  });
});
