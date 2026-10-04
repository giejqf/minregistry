import '@testing-library/jest-dom/vitest';
import { cleanup } from '@testing-library/react';
import { afterEach, vi } from 'vitest';

import { setupApiClient } from '@/lib/api';
import { client } from '@/sdk/client.gen';

// Node's Request needs absolute URLs; the app itself uses same-origin relative ones.
client.setConfig({ baseUrl: 'http://registry.test' });
setupApiClient();

// jsdom lacks these browser APIs used by Radix UI and the theme provider.
if (!window.matchMedia) {
  window.matchMedia = (query: string) =>
    ({
      matches: false,
      media: query,
      onchange: null,
      addEventListener: () => undefined,
      removeEventListener: () => undefined,
      addListener: () => undefined,
      removeListener: () => undefined,
      dispatchEvent: () => false,
    }) as MediaQueryList;
}
globalThis.ResizeObserver ??= class {
  observe() {}
  unobserve() {}
  disconnect() {}
};
for (const name of ['hasPointerCapture', 'releasePointerCapture', 'scrollIntoView'] as const) {
  if (!(name in Element.prototype)) {
    Object.defineProperty(Element.prototype, name, { value: () => false, configurable: true });
  }
}

afterEach(() => {
  cleanup();
  vi.unstubAllGlobals();
  vi.restoreAllMocks();
});
