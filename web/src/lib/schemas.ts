import { z } from 'zod';

/** Same rule as the server (`principals.rs`): it is the `docker login` username. */
export const IDENTITY_NAME_PATTERN = /^[a-z0-9]([a-z0-9._-]{0,62}[a-z0-9])?$/;

export const createIdentitySchema = z.object({
  name: z
    .string()
    .trim()
    .min(1, 'Name is required.')
    .max(64, 'At most 64 characters.')
    .regex(
      IDENTITY_NAME_PATTERN,
      "Lower-case letters, digits, '.', '_' or '-'; must start and end with a letter or digit.",
    ),
  display_name: z.string().trim().max(100, 'At most 100 characters.'),
});

export type CreateIdentityValues = z.infer<typeof createIdentitySchema>;

export const TOKEN_EXPIRY_OPTIONS = [
  { value: 'never', label: 'Never', days: null },
  { value: '7', label: '7 days', days: 7 },
  { value: '30', label: '30 days', days: 30 },
  { value: '90', label: '90 days', days: 90 },
  { value: '365', label: '1 year', days: 365 },
  { value: 'custom', label: 'Custom date…', days: null },
] as const;

export type TokenExpiry = (typeof TOKEN_EXPIRY_OPTIONS)[number]['value'];

const EXPIRY_VALUES = TOKEN_EXPIRY_OPTIONS.map((o) => o.value) as [TokenExpiry, ...TokenExpiry[]];

/** End of the given local calendar day (`YYYY-MM-DD`), or null if invalid. */
function endOfLocalDay(date: string): Date | null {
  const match = /^(\d{4})-(\d{2})-(\d{2})$/.exec(date);
  if (!match) return null;
  const [, y, m, d] = match.map(Number) as [number, number, number, number];
  const result = new Date(y, m - 1, d, 23, 59, 59);
  return Number.isNaN(result.getTime()) || result.getDate() !== d ? null : result;
}

export const createTokenSchema = z
  .object({
    name: z.string().trim().min(1, 'Name is required.').max(64, 'At most 64 characters.'),
    expiry: z.enum(EXPIRY_VALUES),
    customDate: z.string(),
  })
  .superRefine((values, ctx) => {
    if (values.expiry !== 'custom') return;
    const date = endOfLocalDay(values.customDate);
    if (!date) {
      ctx.addIssue({ code: 'custom', path: ['customDate'], message: 'Pick an expiry date.' });
    } else if (date.getTime() <= Date.now()) {
      ctx.addIssue({ code: 'custom', path: ['customDate'], message: 'The expiry date must be in the future.' });
    }
  });

export type CreateTokenValues = z.infer<typeof createTokenSchema>;

/** RFC 3339 `expires_at` for the chosen expiry, or null for "never". */
export function tokenExpiresAt(values: CreateTokenValues, now: Date = new Date()): string | null {
  if (values.expiry === 'never') return null;
  if (values.expiry === 'custom') return endOfLocalDay(values.customDate)?.toISOString() ?? null;
  const days = Number(values.expiry);
  return new Date(now.getTime() + days * 86_400_000).toISOString();
}

export const PERMISSION_LEVELS = ['read', 'write', 'owner'] as const;

export const grantPermissionSchema = z.object({
  principalId: z.string().min(1, 'Choose a principal.'),
  level: z.enum(PERMISSION_LEVELS),
});

export type GrantPermissionValues = z.infer<typeof grantPermissionSchema>;

export const gcSchema = z.object({
  deleteUntagged: z.boolean(),
  minAgeSeconds: z
    .number({ error: 'Enter a number of seconds.' })
    .int('Whole seconds only.')
    .min(0, 'Must not be negative.'),
});

export type GcValues = z.infer<typeof gcSchema>;

/** Audit filter form; `from`/`to` are `datetime-local` values (local time). */
export const auditFiltersSchema = z
  .object({
    principal: z.string().trim(),
    repository: z.string().trim(),
    action: z.string(),
    outcome: z.enum(['', 'ok', 'denied', 'error']),
    from: z.string(),
    to: z.string(),
  })
  .refine((v) => !v.from || !v.to || new Date(v.from).getTime() < new Date(v.to).getTime(), {
    path: ['to'],
    message: '“To” must be after “From”.',
  });

export type AuditFilterValues = z.infer<typeof auditFiltersSchema>;
