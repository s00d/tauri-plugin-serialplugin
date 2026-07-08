import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { defineConfig } from 'vite';
import vue from '@vitejs/plugin-vue';

const __dirname = path.dirname(fileURLToPath(import.meta.url));
/** Repo root (guest-js lives here; linked `file:` dep must not use stale dist-js in dev). */
const repoRoot = path.resolve(__dirname, '../..');

const host = process.env.TAURI_DEV_HOST;

export default defineConfig(({ command }) => ({
  plugins: [vue()],
  clearScreen: false,
  resolve: {
    alias:
      command === 'serve'
        ? {
            'tauri-plugin-serialplugin-api': path.join(repoRoot, 'guest-js/index.ts'),
          }
        : undefined,
    conditions:
      command === 'serve'
        ? ['development', 'import', 'module', 'browser', 'default']
        : ['import', 'module', 'browser', 'production', 'default'],
  },
  optimizeDeps: {
    // Local plugin: skip pre-bundle cache (avoids stale exports + slow re-optimizes after rebuild).
    exclude: ['tauri-plugin-serialplugin-api'],
  },
  server: {
    fs: {
      allow: [repoRoot],
    },
    host: host || false,
    port: 1420,
    strictPort: true,
    hmr: host
      ? {
          protocol: 'ws',
          host: host,
          port: 1430,
        }
      : undefined,
  },
}));
