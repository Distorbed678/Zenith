import { unified } from 'unified';
import remarkParse from 'remark-parse';
import remarkGfm from 'remark-gfm';
import remarkRehype from 'remark-rehype';
import rehypeSlug from 'rehype-slug';
import rehypeStringify from 'rehype-stringify';
import { visit } from 'unist-util-visit';
import { createHighlighter, type Highlighter } from 'shiki';
import type { Element, ElementContent, Root, Text } from 'hast';

export interface TocHeading {
  id: string;
  text: string;
  depth: 2 | 3;
}

export interface RenderedMarkdown {
  html: string;
  headings: TocHeading[];
}

export type LinkResolver = (href: string) => string;

let highlighterPromise: Promise<Highlighter> | null = null;

const PRELOADED_LANGS = [
  'rust',
  'toml',
  'json',
  'bash',
  'sh',
  'yaml',
  'typescript',
  'javascript',
  'svelte',
  'css',
  'html',
  'markdown',
  'sql',
  'diff',
  'glsl',
  'wit',
  'text'
];

async function getHighlighter(): Promise<Highlighter> {
  if (!highlighterPromise) {
    highlighterPromise = createHighlighter({
      themes: ['tokyo-night', 'github-light'],
      langs: PRELOADED_LANGS
    });
  }
  return highlighterPromise;
}

function extractText(node: ElementContent | Element): string {
  if (node.type === 'text') return node.value;
  if ('children' in node && Array.isArray(node.children)) {
    return node.children.map((child) => extractText(child)).join('');
  }
  return '';
}

function resolveLang(raw: string | undefined, loaded: Set<string>): string {
  if (!raw) return 'text';
  const normalized = raw.toLowerCase().trim();
  const aliasMap: Record<string, string> = {
    rs: 'rust',
    ts: 'typescript',
    js: 'javascript',
    yml: 'yaml',
    shell: 'bash',
    zsh: 'bash',
    md: 'markdown',
    txt: 'text',
    plaintext: 'text'
  };
  const candidate = aliasMap[normalized] ?? normalized;
  return loaded.has(candidate) ? candidate : 'text';
}

export async function renderMarkdown(
  source: string,
  resolveLink?: LinkResolver
): Promise<RenderedMarkdown> {
  const highlighter = await getHighlighter();
  const loadedLangs = new Set(highlighter.getLoadedLanguages());
  const headings: TocHeading[] = [];

  const processor = unified()
    .use(remarkParse)
    .use(remarkGfm)
    .use(remarkRehype, { allowDangerousHtml: false })
    .use(rehypeSlug)
    .use(() => (tree: Root) => {
      // 1. Collect H2 and H3 headings for the in-page TOC
      visit(tree, 'element', (node: Element) => {
        if (node.tagName === 'h2' || node.tagName === 'h3') {
          const id = typeof node.properties?.id === 'string' ? node.properties.id : '';
          const text = extractText(node).trim();
          if (id && text) {
            headings.push({
              id,
              text,
              depth: node.tagName === 'h2' ? 2 : 3
            });
          }
        }
      });

      // 2. Rewrite relative .md links to site routes
      if (resolveLink) {
        visit(tree, 'element', (node: Element) => {
          if (node.tagName === 'a' && typeof node.properties?.href === 'string') {
            node.properties.href = resolveLink(node.properties.href);
            if (/^https?:\/\//i.test(node.properties.href)) {
              node.properties.target = '_blank';
              node.properties.rel = ['noopener', 'noreferrer'];
            }
          }
        });
      }

      // 3. Wrap tables in horizontal scroll containers for 360px viewports
      visit(tree, 'element', (node: Element, index, parent) => {
        if (node.tagName === 'table' && parent && typeof index === 'number') {
          const wrapper: Element = {
            type: 'element',
            tagName: 'div',
            properties: { className: ['z-table-scroll'] },
            children: [node]
          };
          parent.children[index] = wrapper;
          return index + 1;
        }
      });

      // 4. Syntax-highlight fenced code blocks with Shiki
      visit(tree, 'element', (node: Element, index, parent) => {
        if (
          node.tagName === 'pre' &&
          parent &&
          typeof index === 'number' &&
          node.children.length === 1 &&
          node.children[0].type === 'element' &&
          node.children[0].tagName === 'code'
        ) {
          const codeEl = node.children[0];
          const classList = Array.isArray(codeEl.properties?.className)
            ? codeEl.properties.className.map(String)
            : [];
          const langClass = classList.find((c) => c.startsWith('language-'));
          const rawLang = langClass?.replace(/^language-/, '');
          const lang = resolveLang(rawLang, loadedLangs);
          const codeText = codeEl.children
            .filter((c): c is Text => c.type === 'text')
            .map((c) => c.value)
            .join('');

          const hastRoot = highlighter.codeToHast(codeText.replace(/\n$/, ''), {
            lang,
            themes: {
              dark: 'tokyo-night',
              light: 'github-light'
            },
            defaultColor: false
          });

          const preEl = hastRoot.children.find(
            (child): child is Element => child.type === 'element' && child.tagName === 'pre'
          );

          if (preEl) {
            preEl.properties = {
              ...preEl.properties,
              'data-language': rawLang || lang,
              tabindex: '0'
            };
            parent.children[index] = preEl;
          }
        }
      });
    })
    .use(rehypeStringify);

  const file = await processor.process(source);
  return {
    html: String(file),
    headings
  };
}
