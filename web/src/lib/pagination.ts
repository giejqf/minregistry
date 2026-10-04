/** Offset pagination state derived from a 1-based `page` search param. */
export interface OffsetPage {
  page: number;
  offset: number;
  limit: number;
}

export function parsePage(raw: string | null, limit: number): OffsetPage {
  const n = Number(raw);
  const page = Number.isInteger(n) && n >= 1 ? n : 1;
  return { page, offset: (page - 1) * limit, limit };
}

export function pageCount(total: number, limit: number): number {
  return Math.max(1, Math.ceil(total / limit));
}

/** "51–100 of 230" */
export function rangeLabel(offset: number, shown: number, total: number): string {
  if (total === 0 || shown === 0) return `0 of ${total}`;
  return `${offset + 1}–${offset + shown} of ${total}`;
}
