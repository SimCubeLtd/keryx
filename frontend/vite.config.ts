import { defineConfig, lazyPlugins } from 'vite-plus'
import { svelte } from '@sveltejs/vite-plugin-svelte'
import tailwindcss from '@tailwindcss/vite'
import { fileURLToPath } from 'node:url'

// Vite Plus reads the pinned pnpm packageManager from package.json.
export default defineConfig({
  plugins: lazyPlugins(() => [svelte(), tailwindcss()]),
  resolve: { alias: { $lib: fileURLToPath(new URL('./src/lib', import.meta.url)) } },
  base: '/',
  build: { outDir: 'dist', emptyOutDir: true },
  server: {
    host: '127.0.0.1',
    strictPort: true,
    proxy: Object.fromEntries(
      ['/api', '/d', '/healthz', '/sw.js', '/manifest.webmanifest', '/pwa-icon'].map((path) => [
        path,
        { target: 'http://127.0.0.1:17823', timeout: 0, proxyTimeout: 0 },
      ]),
    ),
  },
  lint: {
    ignorePatterns: ['src/lib/generated/**', 'dist/**'],
    rules: { 'typescript/no-explicit-any': 'error' },
  },
  fmt: {
    semi: false,
    singleQuote: true,
    ignorePatterns: ['src/lib/generated/**', 'pnpm-lock.yaml', 'dist/**'],
    svelte: true,
  },
  test: { include: ['src/**/*.test.ts'] },
})
