import path from 'node:path';

import tailwindcss from '@tailwindcss/vite';
import react from '@vitejs/plugin-react';
import { defineConfig } from 'vitest/config';

// The Rust server serves the API, the registry and the OAuth flow; in
// development Vite proxies them so the UI runs same-origin, like in production.
const backend = process.env.MINREGISTRY_DEV_BACKEND ?? 'http://localhost:5000';
const proxied = ['/api', '/v2', '/auth', '/healthz', '/readyz'];

export default defineConfig({
  base: '/',
  plugins: [react(), tailwindcss()],
  resolve: {
    alias: {
      '@': path.resolve(import.meta.dirname, './src'),
    },
  },
  server: {
    proxy: Object.fromEntries(proxied.map((prefix) => [prefix, { target: backend, changeOrigin: false }])),
  },
  build: {
    outDir: 'dist',
    emptyOutDir: true,
    rolldownOptions: {
      output: {
        // Long-lived vendor chunks: the app chunk changes on every release, these rarely do.
        codeSplitting: {
          groups: [
            { name: 'react', test: /node_modules[\\/](react|react-dom|scheduler|react-router)[\\/]/ },
            { name: 'ui', test: /node_modules[\\/](radix-ui|@radix-ui|lucide-react|sonner|cn)[\\/]/ },
            { name: 'data', test: /node_modules[\\/](@tanstack|react-hook-form|@hookform|zod)[\\/]/ },
          ],
        },
      },
    },
  },
  test: {
    environment: 'jsdom',
    globals: true,
    setupFiles: ['./src/test/setup.ts'],
    include: ['src/**/*.test.{ts,tsx}'],
    css: false,
  },
});
