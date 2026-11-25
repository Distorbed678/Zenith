import { error } from '@sveltejs/kit';
import { getAllDocSlugs, loadDocBySlug } from '$lib/server/docs';

export function entries() {
  return getAllDocSlugs().map((slug) => ({ slug }));
}

export async function load({ params }: { params: { slug: string } }) {
  const doc = await loadDocBySlug(params.slug);
  if (!doc) {
    error(404, 'Document not found');
  }
  return { doc };
}
