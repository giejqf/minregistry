import type { PermissionLevel } from '@/sdk/types.gen';

export const LEVEL_DESCRIPTIONS: Record<PermissionLevel, string> = {
  read: 'Pull images and list tags.',
  write: 'Read, plus push and delete manifests and tags.',
  owner: 'Write, plus manage this repository’s permissions and delete it.',
};
