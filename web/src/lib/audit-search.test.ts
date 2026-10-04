import { describe, expect, it } from 'vitest';

import {
  auditQuery,
  hasNewerPage,
  newerPage,
  olderPage,
  parseAuditSearch,
  withFilters,
  EMPTY_AUDIT_FILTERS,
} from '@/lib/audit-search';

describe('audit search params', () => {
  it('parses filters and ignores unknown outcomes', () => {
    const page = parseAuditSearch(new URLSearchParams('principal=ci&outcome=nope&action=manifest.push'));
    expect(page.filters).toEqual({ ...EMPTY_AUDIT_FILTERS, principal: 'ci', action: 'manifest.push' });
    expect(page.cursor).toBe('');
    expect(page.prev).toEqual([]);
  });

  it('walks older and newer pages with a cursor stack', () => {
    const first = parseAuditSearch(withFilters({ ...EMPTY_AUDIT_FILTERS, outcome: 'denied' }));
    expect(hasNewerPage(first)).toBe(false);

    const second = parseAuditSearch(olderPage(first, '100'));
    expect(second).toMatchObject({ cursor: '100', prev: [''] });
    expect(second.filters.outcome).toBe('denied');
    expect(hasNewerPage(second)).toBe(true);

    const third = parseAuditSearch(olderPage(second, '50'));
    expect(third).toMatchObject({ cursor: '50', prev: ['', '100'] });

    const back = parseAuditSearch(newerPage(third));
    expect(back).toMatchObject({ cursor: '100', prev: [''] });

    const top = newerPage(back);
    expect(top.toString()).toBe('outcome=denied');
  });

  it('new filters reset pagination', () => {
    const params = withFilters({ ...EMPTY_AUDIT_FILTERS, repository: 'team/app' });
    expect(params.toString()).toBe('repository=team%2Fapp');
  });

  it('builds the API query without empty values', () => {
    const page = parseAuditSearch(new URLSearchParams('repository=app&from=2026-10-01T00%3A00%3A00.000Z&cursor=42'));
    expect(auditQuery(page)).toEqual({ limit: 50, repository: 'app', from: '2026-10-01T00:00:00.000Z', cursor: '42' });
  });
});
