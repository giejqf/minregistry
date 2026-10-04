import { useContext } from 'react';

import { ThemeProviderContext } from '@/lib/theme';

export function useTheme() {
  return useContext(ThemeProviderContext);
}
