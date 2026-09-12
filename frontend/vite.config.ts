import { defineConfig } from 'vitest/config';
import react from '@vitejs/plugin-react';

export default defineConfig({
  plugins: [react()],
  server: {
    port: 5173,
    // Dev only. In production the Rust binary serves both the API and this bundle
    // from one origin, so there is no CORS anywhere.
    proxy: { '/api': { target: 'http://localhost:3100', changeOrigin: true } },
  },
  test: {
    environment: 'jsdom',
    setupFiles: './src/test/setup.ts',
    css: true,
  },
});
