import type { CreateClientConfig } from '@/sdk/client.gen';

export const createClientConfig: CreateClientConfig = (config) => ({
  ...config,
  baseUrl: '',
  credentials: 'same-origin',
  headers: { 'X-Requested-With': 'XMLHttpRequest' },
});
