import { CircleHelp } from 'lucide-react';
import { Link } from 'react-router';

import { EmptyState } from '@/components/query-state';
import { Button } from '@/components/ui/button';

export function NotFoundPage() {
  return (
    <EmptyState icon={CircleHelp} title="Page not found" description="There is nothing at this address.">
      <Button variant="outline" asChild>
        <Link to="/repositories">Go to repositories</Link>
      </Button>
    </EmptyState>
  );
}
