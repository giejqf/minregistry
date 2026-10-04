import type { AuditOutcome, ListAuditEventsData } from '@/sdk/types.gen';

export const AUDIT_OUTCOMES: readonly AuditOutcome[] = ['ok', 'denied', 'error'];
export const AUDIT_PAGE_SIZE = 50;

export interface AuditFilters {
  principal: string;
  repository: string;
  action: string;
  outcome: AuditOutcome | '';
  /** RFC 3339 */
  from: string;
  /** RFC 3339 */
  to: string;
}

export const EMPTY_AUDIT_FILTERS: AuditFilters = {
  principal: '',
  repository: '',
  action: '',
  outcome: '',
  from: '',
  to: '',
};

const FILTER_KEYS = ['principal', 'repository', 'action', 'outcome', 'from', 'to'] as const;

/** The first page is written as this marker in the `prev` stack. */
const FIRST_PAGE = '-';

export interface AuditPage {
  filters: AuditFilters;
  /** `cursor` of the page on screen; '' for the newest page. */
  cursor: string;
  /** Cursors of the newer pages visited before this one, oldest last. */
  prev: string[];
}

function isOutcome(value: string): value is AuditOutcome {
  return (AUDIT_OUTCOMES as readonly string[]).includes(value);
}

export function parseAuditSearch(params: URLSearchParams): AuditPage {
  const get = (key: string) => params.get(key)?.trim() ?? '';
  const outcome = get('outcome');
  const prevRaw = params.get('prev');
  return {
    filters: {
      principal: get('principal'),
      repository: get('repository'),
      action: get('action'),
      outcome: isOutcome(outcome) ? outcome : '',
      from: get('from'),
      to: get('to'),
    },
    cursor: get('cursor'),
    prev: prevRaw === null || prevRaw === '' ? [] : prevRaw.split(',').map((c) => (c === FIRST_PAGE ? '' : c)),
  };
}

export function buildAuditSearch(page: AuditPage): URLSearchParams {
  const params = new URLSearchParams();
  for (const key of FILTER_KEYS) {
    const value = page.filters[key];
    if (value) params.set(key, value);
  }
  if (page.cursor) params.set('cursor', page.cursor);
  if (page.prev.length) params.set('prev', page.prev.map((c) => c || FIRST_PAGE).join(','));
  return params;
}

/** New filters always start again at the newest page. */
export function withFilters(filters: AuditFilters): URLSearchParams {
  return buildAuditSearch({ filters, cursor: '', prev: [] });
}

export function olderPage(page: AuditPage, nextCursor: string): URLSearchParams {
  return buildAuditSearch({ ...page, cursor: nextCursor, prev: [...page.prev, page.cursor] });
}

export function newerPage(page: AuditPage): URLSearchParams {
  const prev = page.prev.slice(0, -1);
  const cursor = page.prev.length ? (page.prev[page.prev.length - 1] ?? '') : '';
  return buildAuditSearch({ ...page, cursor, prev });
}

export function hasNewerPage(page: AuditPage): boolean {
  return page.cursor !== '' || page.prev.length > 0;
}

export function hasActiveFilters(filters: AuditFilters): boolean {
  return FILTER_KEYS.some((key) => filters[key] !== '');
}

/** Query parameters for `GET /api/v1/audit`, without empty values. */
export function auditQuery(page: AuditPage, limit = AUDIT_PAGE_SIZE): NonNullable<ListAuditEventsData['query']> {
  const query: NonNullable<ListAuditEventsData['query']> = { limit };
  const { filters } = page;
  if (filters.principal) query.principal = filters.principal;
  if (filters.repository) query.repository = filters.repository;
  if (filters.action) query.action = filters.action;
  if (filters.outcome) query.outcome = filters.outcome;
  if (filters.from) query.from = filters.from;
  if (filters.to) query.to = filters.to;
  if (page.cursor) query.cursor = page.cursor;
  return query;
}
