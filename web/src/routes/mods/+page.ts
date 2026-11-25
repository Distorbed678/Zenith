import { base } from '$app/paths';

export type ModPermission = 'network' | 'filesystem' | 'render';

export interface ModIndexEntry {
  id: string;
  name: string;
  version: string;
  author: string;
  summary: string;
  tags: string[];
  permissions: ModPermission[];
  targetVersion: string;
  example?: boolean;
}

export interface ModIndexPayload {
  schemaVersion: number;
  status: string;
  note: string;
  mods: ModIndexEntry[];
}

export async function load({ fetch }: { fetch: typeof globalThis.fetch }) {
  const response = await fetch(`${base}/mod-index.sample.json`);
  const payload = (await response.json()) as ModIndexPayload;
  return { payload };
}
