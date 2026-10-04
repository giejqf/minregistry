import {
  Container,
  FolderGit2,
  KeyRound,
  LogOut,
  ScrollText,
  Server,
  ShieldCheck,
  UserRound,
  Users,
} from 'lucide-react';
import { Link, NavLink, Outlet } from 'react-router';

import { ModeToggle } from '@/components/mode-toggle';
import { Button } from '@/components/ui/button';
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuGroup,
  DropdownMenuItem,
  DropdownMenuLabel,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu';
import { useAuth } from '@/hooks/use-auth';
import { cn } from '@/lib/utils';

const NAV = [
  { to: '/repositories', label: 'Repositories', icon: FolderGit2 },
  { to: '/principals', label: 'Principals', icon: Users },
  { to: '/permissions', label: 'Permissions', icon: ShieldCheck },
  { to: '/audit', label: 'Audit log', icon: ScrollText },
  { to: '/system', label: 'System', icon: Server },
] as const;

function initials(name: string): string {
  const parts = name.split(/[\s._-]+/).filter(Boolean);
  return ((parts[0]?.[0] ?? '') + (parts[1]?.[0] ?? '')).toUpperCase() || '?';
}

function UserMenu() {
  const { me, signOut } = useAuth();
  return (
    <DropdownMenu>
      <DropdownMenuTrigger asChild>
        <Button
          variant="ghost"
          className="h-auto min-w-0 flex-1 justify-start gap-2 px-2 py-1.5"
          aria-label="Account menu"
        >
          <span className="flex size-7 shrink-0 items-center justify-center rounded-full bg-primary text-xs font-medium text-primary-foreground">
            {initials(me.display_name || me.name)}
          </span>
          <span className="min-w-0 text-left">
            <span className="block truncate text-sm font-medium">{me.display_name || me.name}</span>
            <span className="block truncate text-xs text-muted-foreground">@{me.name}</span>
          </span>
        </Button>
      </DropdownMenuTrigger>
      <DropdownMenuContent align="start" side="top" className="w-56">
        <DropdownMenuLabel className="font-normal">
          Signed in as <span className="font-medium">{me.name}</span>
        </DropdownMenuLabel>
        <DropdownMenuSeparator />
        <DropdownMenuGroup>
          <DropdownMenuItem asChild>
            <Link to={`/principals/${me.id}`}>
              <UserRound /> My principal
            </Link>
          </DropdownMenuItem>
          <DropdownMenuItem asChild>
            <Link to={`/principals/${me.id}?issue=1`}>
              <KeyRound /> Create my CLI token
            </Link>
          </DropdownMenuItem>
        </DropdownMenuGroup>
        <DropdownMenuSeparator />
        <DropdownMenuItem onSelect={() => void signOut()}>
          <LogOut /> Sign out
        </DropdownMenuItem>
      </DropdownMenuContent>
    </DropdownMenu>
  );
}

export function AppShell() {
  return (
    <div className="flex min-h-svh">
      <aside className="sticky top-0 flex h-svh w-56 shrink-0 flex-col border-r bg-sidebar text-sidebar-foreground">
        <Link to="/repositories" className="flex items-center gap-2 px-4 py-4 font-semibold">
          <span className="flex size-7 items-center justify-center rounded-md bg-primary text-primary-foreground">
            <Container className="size-4" />
          </span>
          MinRegistry
        </Link>
        <nav className="flex flex-1 flex-col gap-0.5 px-2" aria-label="Main">
          {NAV.map(({ to, label, icon: Icon }) => (
            <NavLink
              key={to}
              to={to}
              className={({ isActive }) =>
                cn(
                  'flex items-center gap-2 rounded-md px-2 py-1.5 text-sm text-sidebar-foreground/80 transition-colors hover:bg-sidebar-accent hover:text-sidebar-accent-foreground',
                  isActive && 'bg-sidebar-accent font-medium text-sidebar-accent-foreground',
                )
              }
            >
              <Icon className="size-4" />
              {label}
            </NavLink>
          ))}
        </nav>
        <div className="flex items-center gap-1 border-t p-2">
          <UserMenu />
          <ModeToggle />
        </div>
      </aside>
      <main className="min-w-0 flex-1">
        <div className="mx-auto max-w-6xl space-y-6 px-6 py-8">
          <Outlet />
        </div>
      </main>
    </div>
  );
}
