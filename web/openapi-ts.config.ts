import { defineConfig } from '@hey-api/openapi-ts';

// Generates src/sdk/ from openapi.json (the backend's `minregistry openapi`
// output). The generated files are committed and must not be edited by hand:
// run `pnpm gen:sdk` after the backend's API changes.
export default defineConfig({
  input: './openapi.json',
  output: {
    path: './src/sdk',
    tsConfigPath: './tsconfig.app.json',
  },
  plugins: [
    {
      name: '@hey-api/client-fetch',
      // Same-origin requests with the CSRF header the API requires; see the file.
      runtimeConfigPath: './src/lib/api-client-config.ts',
    },
    '@hey-api/typescript',
    '@hey-api/sdk',
    '@tanstack/react-query',
  ],
});
