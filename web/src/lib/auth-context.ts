import { createContext } from 'react';

import type { PrincipalSummary } from '@/sdk/types.gen';

export interface AuthState {
  /** The signed-in admin's own (GitHub) principal. */
  me: PrincipalSummary;
  signOut: () => Promise<void>;
}

export const AuthContext = createContext<AuthState | null>(null);
