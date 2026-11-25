import { sveltekit } from '@sveltejs/kit/vite';
import { defineConfig } from 'vite';
import path from 'node:path';

const repoRoot = path.resolve(import.meta.dirname, '..');

export default defineConfig({
  plugins: [sveltekit()],
  server: {
    fs: {
      allow: [repoRoot]
    }
  }
});
