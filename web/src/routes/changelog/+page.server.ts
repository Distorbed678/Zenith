import { loadChangelog } from '$lib/server/docs';

export async function load() {
  const changelog = await loadChangelog();
  return { changelog };
}
