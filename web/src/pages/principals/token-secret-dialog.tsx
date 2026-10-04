import { TriangleAlert } from 'lucide-react';

import { CommandSnippet } from '@/components/command-snippet';
import { Alert, AlertDescription, AlertTitle } from '@/components/ui/alert';
import { Button } from '@/components/ui/button';
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog';
import { dockerLoginSnippet, registryHost } from '@/lib/format';
import type { CreateTokenResponse } from '@/sdk/types.gen';

/** Shows a freshly issued token's secret, which the server never returns again. */
export function TokenSecretDialog({ created, onClose }: { created: CreateTokenResponse | null; onClose: () => void }) {
  const host = registryHost();
  return (
    <Dialog open={created !== null} onOpenChange={(open) => !open && onClose()}>
      {created && (
        <DialogContent className="sm:max-w-xl" onInteractOutside={(e) => e.preventDefault()}>
          <DialogHeader>
            <DialogTitle>Token “{created.token.name}” created</DialogTitle>
            <DialogDescription>
              Username <code className="font-mono text-foreground">{created.username}</code>
            </DialogDescription>
          </DialogHeader>
          <Alert>
            <TriangleAlert />
            <AlertTitle>Copy the token now — you won’t see it again.</AlertTitle>
            <AlertDescription>
              MinRegistry stores only a hash of it. If you lose it, revoke it and issue a new one.
            </AlertDescription>
          </Alert>
          <div className="space-y-1.5">
            <div className="text-sm font-medium">Token</div>
            <CommandSnippet command={created.secret} label="token" />
          </div>
          <div className="space-y-1.5">
            <div className="text-sm font-medium">Log in with Docker</div>
            <CommandSnippet
              command={dockerLoginSnippet(host, created.username, created.secret)}
              label="docker login command"
            />
          </div>
          <DialogFooter>
            <Button onClick={onClose}>Done</Button>
          </DialogFooter>
        </DialogContent>
      )}
    </Dialog>
  );
}
