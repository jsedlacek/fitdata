import { defineConfig } from 'vite';
import react from '@vitejs/plugin-react';
import tailwindcss from '@tailwindcss/vite';
import type { Connect, Plugin } from 'vite';

/** In `vite dev` and `vite preview`, /docs and /docs/<page> get docs.html, as from the Rust server. */
function docsPages(): Plugin {
  const rewrite: Connect.NextHandleFunction = (req, _res, next) => {
    const r = req as { url?: string };
    if (r.url && /^\/docs(\/[^.]*)?(\?.*)?$/.test(r.url)) r.url = '/docs.html';
    next();
  };
  return {
    name: 'docs-pages',
    configureServer: (s) => void s.middlewares.use(rewrite),
    configurePreviewServer: (s) => void s.middlewares.use(rewrite),
  };
}

// In development the Rust server (`fitdata serve`, port 8080) provides /api and /auth.
// Besides the dashboard (index.html) the build has the public pages: the home page for
// signed-out visitors, the privacy policy and the terms. They are plain HTML with the
// dashboard's stylesheet and no script, so they load fast and read without JavaScript.
// The docs (docs.html, served at /docs/...) are a small app of their own, public too.
export default defineConfig({
  plugins: [react(), tailwindcss(), docsPages()],
  resolve: { alias: { '@': new URL('./src', import.meta.url).pathname } },
  server: {
    port: 5173,
    proxy: { '/api': 'http://localhost:8080', '/auth': 'http://localhost:8080' },
  },
  build: {
    // One app chunk (React, Radix, TanStack) of about 160 kB gzipped; splitting it gains little.
    chunkSizeWarningLimit: 600,
    rollupOptions: {
      input: {
        index: 'index.html',
        home: 'home.html',
        privacy: 'privacy.html',
        terms: 'terms.html',
        docs: 'docs.html',
      },
    },
  },
});
