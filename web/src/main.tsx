import { StrictMode } from 'react';
import { createRoot } from 'react-dom/client';
import { BrowserRouter } from 'react-router';

import '@/index.css';
import { App } from '@/app';
import { setupApiClient } from '@/lib/api';
import { createQueryClient } from '@/lib/query-client';
import { Providers } from '@/providers';

setupApiClient();
const queryClient = createQueryClient();

const root = document.getElementById('root');
if (!root) throw new Error('missing #root element');

createRoot(root).render(
  <StrictMode>
    <Providers queryClient={queryClient}>
      <BrowserRouter>
        <App />
      </BrowserRouter>
    </Providers>
  </StrictMode>,
);
