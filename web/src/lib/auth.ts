/**
 * OAuth navigation and sign-out are plain browser requests to `/auth/*`,
 * which is not part of the management API (and not in the SDK).
 */

/** The GitHub sign-in URL that returns to `next` (a same-origin path) afterwards. */
export function signInUrl(next: string): string {
  const safe = next.startsWith('/') && !next.startsWith('//') && !next.startsWith('/auth/') ? next : '/';
  return safe === '/' ? '/auth/github/login' : `/auth/github/login?next=${encodeURIComponent(safe)}`;
}

export function currentPath(): string {
  return `${window.location.pathname}${window.location.search}`;
}

/** Full-page navigation into the GitHub OAuth flow. */
export function startSignIn(next: string = currentPath()): void {
  window.location.assign(signInUrl(next));
}

/** Ends the admin session. Resolves when the server confirmed it. */
export async function signOut(): Promise<void> {
  const res = await fetch('/auth/logout', {
    method: 'POST',
    credentials: 'same-origin',
    headers: { 'X-Requested-With': 'XMLHttpRequest' },
  });
  if (!res.ok && res.status !== 401) {
    throw new Error(`Sign-out failed (HTTP ${res.status}).`);
  }
}
