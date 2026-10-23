import { defineConfig } from 'vite';
import react from '@vitejs/plugin-react';
import tailwindcss from '@tailwindcss/vite';

// In development the Rust server (`fitdata serve`, port 8080) provides /api and /auth.
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
      output: {
        // ECharts is most of the bundle and changes rarely: its own long-cached chunk.
        manualChunks: (id) => (/node_modules\/(echarts|zrender)\//.test(id) ? 'echarts' : undefined),
      },
    },
  },
});
