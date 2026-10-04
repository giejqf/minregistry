import { Navigate, Route, Routes } from 'react-router';

import { AppShell } from '@/components/app-shell';
import { AuthGate } from '@/components/auth-gate';
import { AuditPage } from '@/pages/audit-page';
import { NotFoundPage } from '@/pages/not-found-page';
import { PermissionsPage } from '@/pages/permissions/permissions-page';
import { RepositoryPermissionsPage } from '@/pages/permissions/repository-permissions-page';
import { PrincipalDetailPage } from '@/pages/principals/principal-detail-page';
import { PrincipalsPage } from '@/pages/principals/principals-page';
import { RepositoriesPage } from '@/pages/repositories/repositories-page';
import { RepositoryDetailPage } from '@/pages/repositories/repository-detail-page';
import { SystemPage } from '@/pages/system-page';

export function AppRoutes() {
  return (
    <Routes>
      <Route element={<AppShell />}>
        <Route index element={<Navigate to="/repositories" replace />} />
        <Route path="repositories" element={<RepositoriesPage />} />
        <Route path="repositories/:id" element={<RepositoryDetailPage />} />
        <Route path="repositories/:id/permissions" element={<RepositoryPermissionsPage />} />
        <Route path="principals" element={<PrincipalsPage />} />
        <Route path="principals/:id" element={<PrincipalDetailPage />} />
        <Route path="permissions" element={<PermissionsPage />} />
        <Route path="audit" element={<AuditPage />} />
        <Route path="system" element={<SystemPage />} />
        <Route path="*" element={<NotFoundPage />} />
      </Route>
    </Routes>
  );
}

export function App() {
  return (
    <AuthGate>
      <AppRoutes />
    </AuthGate>
  );
}
