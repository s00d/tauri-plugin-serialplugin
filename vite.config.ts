import { defineConfig } from 'vite'

// Library build of guest-js: ESM + CJS, @tauri-apps/api stays external.
// Type declarations come from `tsc -p tsconfig.build.json`.
export default defineConfig({
  build: {
    outDir: 'dist-js',
    emptyOutDir: true,
    minify: false,
    sourcemap: false,
    target: 'es2021',
    lib: {
      entry: 'guest-js/index.ts',
      formats: ['es', 'cjs'],
      fileName: (format) => (format === 'es' ? 'index.js' : 'index.cjs')
    },
    rolldownOptions: {
      external: [/^@tauri-apps\/api/]
    }
  }
})
