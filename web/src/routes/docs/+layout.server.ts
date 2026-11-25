import { getDocSidebarGroups } from '$lib/server/docs';

export function load() {
  return {
    groups: getDocSidebarGroups()
  };
}
