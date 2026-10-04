import { afterEach, describe, expect, it, vi } from 'vitest';

import { auditFiltersSchema, createIdentitySchema, createTokenSchema, gcSchema, tokenExpiresAt } from '@/lib/schemas';

describe('createIdentitySchema', () => {
  it.each(['ci', 'ci-deploy', 'alice.smith', 'a', 'build_bot2'])('accepts %s', (name) => {
    expect(createIdentitySchema.safeParse({ name, display_name: '' }).success).toBe(true);
  });

  it.each(['', 'CI', '-ci', 'ci-', 'has space', 'a/b', 'x'.repeat(65)])('rejects %j', (name) => {
    expect(createIdentitySchema.safeParse({ name, display_name: '' }).success).toBe(false);
  });

  it('trims the name', () => {
    expect(createIdentitySchema.parse({ name: '  ci  ', display_name: '' }).name).toBe('ci');
  });
});

describe('createTokenSchema', () => {
  afterEach(() => {
    vi.useRealTimers();
  });

  it('requires a name', () => {
    expect(createTokenSchema.safeParse({ name: ' ', expiry: 'never', customDate: '' }).success).toBe(false);
  });

  it('requires a future custom date', () => {
    vi.useFakeTimers({ now: new Date(2026, 9, 3, 12, 0) });
    expect(createTokenSchema.safeParse({ name: 't', expiry: 'custom', customDate: '' }).success).toBe(false);
    expect(createTokenSchema.safeParse({ name: 't', expiry: 'custom', customDate: '2026-10-02' }).success).toBe(false);
    expect(createTokenSchema.safeParse({ name: 't', expiry: 'custom', customDate: '2026-10-03' }).success).toBe(true);
  });

  it('computes expires_at', () => {
    const now = new Date('2026-10-03T00:00:00Z');
    expect(tokenExpiresAt({ name: 't', expiry: 'never', customDate: '' }, now)).toBeNull();
    expect(tokenExpiresAt({ name: 't', expiry: '30', customDate: '' }, now)).toBe('2026-11-02T00:00:00.000Z');
    expect(tokenExpiresAt({ name: 't', expiry: 'custom', customDate: '2026-12-31' }, now)).toBe(
      new Date(2026, 11, 31, 23, 59, 59).toISOString(),
    );
  });
});

describe('gcSchema', () => {
  it('accepts whole non-negative seconds only', () => {
    expect(gcSchema.safeParse({ deleteUntagged: false, minAgeSeconds: 0 }).success).toBe(true);
    expect(gcSchema.safeParse({ deleteUntagged: false, minAgeSeconds: -1 }).success).toBe(false);
    expect(gcSchema.safeParse({ deleteUntagged: false, minAgeSeconds: 1.5 }).success).toBe(false);
    expect(gcSchema.safeParse({ deleteUntagged: false, minAgeSeconds: Number.NaN }).success).toBe(false);
  });
});

describe('auditFiltersSchema', () => {
  const base = { principal: '', repository: '', action: '', outcome: '' as const, from: '', to: '' };
  it('requires to after from', () => {
    expect(auditFiltersSchema.safeParse({ ...base, from: '2026-10-02T00:00', to: '2026-10-01T00:00' }).success).toBe(
      false,
    );
    expect(auditFiltersSchema.safeParse({ ...base, from: '2026-10-01T00:00', to: '2026-10-02T00:00' }).success).toBe(
      true,
    );
    expect(auditFiltersSchema.safeParse({ ...base, from: '2026-10-01T00:00' }).success).toBe(true);
  });
});
