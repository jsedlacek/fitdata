import { defineConfig } from 'vite';
import react from '@vitejs/plugin-react';
import tailwindcss from '@tailwindcss/vite';

// In development the Rust server (`fitdata serve`, port 8080) provides /api and /auth.
// Besides the dashboard (index.html) the build has the public pages: the home page for
// signed-out visitors, the privacy policy and the terms. They are plain HTML with the
// dashboard's stylesheet and no script, so they load fast and read without JavaScript.
export default defineConfig({
  plugins: [react(), tailwindcss()],
  resolve: { alias: { '@': new URL('./src', import.meta.url).pathname } },
  server: {
    port: 5173,
    proxy: { '/api': 'http://localhost:8080', '/auth': 'http://localhost:8080' },
  },
  build: {
    chunkSizeWarningLimit: 1200,
    rollupOptions: {
      input: {
        index: 'index.html',
        home: 'home.html',
        privacy: 'privacy.html',
        terms: 'terms.html',
      },
      output: {
        // ECharts is most of the bundle and changes rarely: its own long-cached chunk.
        manualChunks: (id) => (/node_modules\/(echarts|zrender)\//.test(id) ? 'echarts' : undefined),
      },
    },
  },
});
