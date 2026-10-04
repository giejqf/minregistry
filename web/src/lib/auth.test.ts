import { describe, expect, it, vi } from 'vitest';

import { signInUrl, signOut } from '@/lib/auth';

describe('signInUrl', () => {
  it('returns to the current page after sign-in', () => {
    expect(signInUrl('/principals/3?issue=1')).toBe('/auth/github/login?next=%2Fprincipals%2F3%3Fissue%3D1');
  });

  it('only accepts same-origin paths', () => {
    expect(signInUrl('/')).toBe('/auth/github/login');
    expect(signInUrl('//evil.example.com')).toBe('/auth/github/login');
    expect(signInUrl('https://evil.example.com')).toBe('/auth/github/login');
    expect(signInUrl('/auth/logout')).toBe('/auth/github/login');
  });
});

describe('signOut', () => {
  it('posts to /auth/logout with the CSRF header', async () => {
    const fetchMock = vi.fn(() => Promise.resolve(new Response(null, { status: 204 })));
    vi.stubGlobal('fetch', fetchMock);
    await signOut();
    expect(fetchMock).toHaveBeenCalledWith('/auth/logout', {
      method: 'POST',
      credentials: 'same-origin',
      headers: { 'X-Requested-With': 'XMLHttpRequest' },
    });
  });

  it('fails loudly when the server refuses', async () => {
    vi.stubGlobal(
      'fetch',
      vi.fn(() => Promise.resolve(new Response(null, { status: 500 }))),
    );
    await expect(signOut()).rejects.toThrow(/HTTP 500/);
  });
});
