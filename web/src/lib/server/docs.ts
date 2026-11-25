import path from 'node:path';
import { base } from '$app/paths';
import { siteStrings } from '$lib/strings';
import { renderMarkdown, type TocHeading } from './markdown';

export type DocGroupKey = 'plan' | 'guide' | 'adr' | 'specs' | 'decisions';

export interface DocMeta {
  slug: string;
  group: DocGroupKey;
  title: string;
  fileName: string;
  repoRelPath: string;
  orderKey: string;
}

export interface DocGroup {
  key: DocGroupKey;
  label: string;
  items: DocMeta[];
}

export interface LoadedDoc extends DocMeta {
  html: string;
  headings: TocHeading[];
}

const rawPlanDocs = import.meta.glob('../../../../docs/plan/*.md', {
  query: '?raw',
  import: 'default',
  eager: true
}) as Record<string, string>;

const rawGuideDocs = import.meta.glob('../../../../docs/guide/*.md', {
  query: '?raw',
  import: 'default',
  eager: true
}) as Record<string, string>;

const rawAdrDocs = import.meta.glob('../../../../docs/adr/*.md', {
  query: '?raw',
  import: 'default',
  eager: true
}) as Record<string, string>;

const rawSpecDocs = import.meta.glob('../../../../docs/specs/*.md', {
  query: '?raw',
  import: 'default',
  eager: true
}) as Record<string, string>;

const rawDecisionDocs = import.meta.glob('../../../../docs/decisions/USER-ANSWERS.md', {
  query: '?raw',
  import: 'default',
  eager: true
}) as Record<string, string>;

const rawChangelogFiles = import.meta.glob('../../../../CHANGELOG.md', {
  query: '?raw',
  import: 'default',
  eager: true
}) as Record<string, string>;

const GROUP_ORDER: DocGroupKey[] = ['plan', 'guide', 'adr', 'specs', 'decisions'];

interface InternalDocEntry extends DocMeta {
  raw: string;
}

function slugifyFileName(fileName: string): string {
  return fileName
    .replace(/\.md$/i, '')
    .toLowerCase()
    .replace(/[^a-z0-9-_]+/g, '-')
    .replace(/^-+|-+$/g, '');
}

function extractTitle(raw: string, fallbackFile: string): string {
  const match = raw.match(/^#\s+(.+)$/m);
  if (match) {
    return match[1].replace(/[*_`]/g, '').trim();
  }
  return fallbackFile
    .replace(/\.md$/i, '')
    .replace(/[-_]+/g, ' ')
    .replace(/^\d+\s+/, '');
}

function collectGroup(group: DocGroupKey, map: Record<string, string>): InternalDocEntry[] {
  return Object.entries(map)
    .map(([globPath, raw]) => {
      const normalized = globPath.replace(/\\/g, '/');
      const docsIdx = normalized.lastIndexOf('/docs/');
      const repoRelPath = docsIdx >= 0 ? normalized.slice(docsIdx + 1) : `docs/${group}/${path.basename(normalized)}`;
      const fileName = path.posix.basename(repoRelPath);
      const slug = `${group}/${slugifyFileName(fileName)}`;
      return {
        slug,
        group,
        title: extractTitle(raw, fileName),
        fileName,
        repoRelPath,
        orderKey: fileName.toLowerCase(),
        raw
      };
    })
    .sort((a, b) => a.orderKey.localeCompare(b.orderKey, undefined, { numeric: true }));
}

const ALL_ENTRIES: InternalDocEntry[] = [
  ...collectGroup('plan', rawPlanDocs),
  ...collectGroup('guide', rawGuideDocs),
  ...collectGroup('adr', rawAdrDocs),
  ...collectGroup('specs', rawSpecDocs),
  ...collectGroup('decisions', rawDecisionDocs)
];

const ENTRY_BY_SLUG = new Map<string, InternalDocEntry>();
const ENTRY_BY_REPO_PATH_LOWER = new Map<string, InternalDocEntry>();

for (const entry of ALL_ENTRIES) {
  ENTRY_BY_SLUG.set(entry.slug, entry);
  ENTRY_BY_REPO_PATH_LOWER.set(entry.repoRelPath.toLowerCase(), entry);
}

export function getDocSidebarGroups(): DocGroup[] {
  return GROUP_ORDER.map((key) => ({
    key,
    label: siteStrings.docs.groups[key],
    items: ALL_ENTRIES.filter((entry) => entry.group === key).map(({ raw: _raw, ...meta }) => meta)
  }));
}

export function getAllDocSlugs(): string[] {
  return ALL_ENTRIES.map((entry) => entry.slug);
}

export function createMarkdownLinkResolver(fromRepoRelPath: string): (href: string) => string {
  const fromDir = path.posix.dirname(fromRepoRelPath);

  return (href: string) => {
    if (!href || href.startsWith('#') || /^[a-z][a-z0-9+.-]*:/i.test(href)) {
      return href;
    }

    const [pathPart, hashPart] = href.split('#');
    const hashSuffix = hashPart ? `#${hashPart}` : '';

    // Keep root-relative site links intact
    if (pathPart.startsWith('/')) {
      return href;
    }

    const candidateA = path.posix.normalize(path.posix.join(fromDir, pathPart));
    const candidateB = path.posix.normalize(pathPart.replace(/^\/+/, ''));

    if (pathPart.toLowerCase().endsWith('.md')) {
      const matched =
        ENTRY_BY_REPO_PATH_LOWER.get(candidateA.toLowerCase()) ??
        ENTRY_BY_REPO_PATH_LOWER.get(candidateB.toLowerCase());

      if (matched) {
        return `${base}/docs/${matched.slug}${hashSuffix}`;
      }

      if (candidateA.toLowerCase() === 'changelog.md' || candidateB.toLowerCase() === 'changelog.md') {
        return `${base}/changelog${hashSuffix}`;
      }
    }

    // Fall back to GitHub source link for relative repo files outside rendered docs (e.g. .toml, .rs, scripts)
    const repoPath = candidateA.replace(/^(\.\.\/)+/, '').replace(/^\/+/, '');
    if (repoPath) {
      return `${siteStrings.brand.repoUrl}/blob/main/${repoPath}${hashSuffix}`;
    }

    return href;
  };
}

export async function loadDocBySlug(slug: string): Promise<LoadedDoc | null> {
  const normalized = slug.replace(/^\/+|\/+$/g, '').toLowerCase();
  const entry = ENTRY_BY_SLUG.get(normalized);
  if (!entry) return null;

  const rendered = await renderMarkdown(entry.raw, createMarkdownLinkResolver(entry.repoRelPath));
  const { raw: _raw, ...meta } = entry;
  return {
    ...meta,
    html: rendered.html,
    headings: rendered.headings
  };
}

export async function loadChangelog(): Promise<{
  exists: boolean;
  html: string;
  headings: TocHeading[];
}> {
  const raw = Object.values(rawChangelogFiles)[0];
  if (!raw || !raw.trim()) {
    return { exists: false, html: '', headings: [] };
  }
  const rendered = await renderMarkdown(raw, createMarkdownLinkResolver('CHANGELOG.md'));
  return {
    exists: true,
    html: rendered.html,
    headings: rendered.headings
  };
}
