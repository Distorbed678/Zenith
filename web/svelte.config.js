import adapter from '@sveltejs/adapter-static';
import { vitePreprocess } from '@sveltejs/vite-plugin-svelte';

const basePath = process.env.BASE_PATH ?? '';

/** @type {import('@sveltejs/kit').Config} */
const config = {
  preprocess: vitePreprocess(),
  kit: {
    adapter: adapter({
      pages: 'build',
      assets: 'build',
      fallback: '404.html',
      precompress: false,
      strict: true
    }),
    paths: {
      base: basePath
    },
    prerender: {
      handleHttpError: ({ path, message }) => {
        // /api/ is populated separately by rustdoc in CI
        if (path.endsWith('/api/') || path.endsWith('/api')) {
          return;
        }
        throw new Error(message);
      },
      handleMissingId: 'warn'
    }
  }
};

export default config;
