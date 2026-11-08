// GitHub Pages serves the site from https://s00d.github.io/tauri-plugin-serialplugin/.
const baseURL = process.env.NUXT_APP_BASE_URL || (process.env.NODE_ENV === 'production' ? '/tauri-plugin-serialplugin/' : '/')

export default defineNuxtConfig({
  modules: ['@nuxt/ui', '@nuxt/content'],

  css: ['~/assets/css/main.css'],

  app: {
    baseURL,
    head: {
      htmlAttrs: { lang: 'en' },
      link: [{ rel: 'icon', type: 'image/svg+xml', href: `${baseURL}favicon.svg` }],
    },
  },

  icon: {
    // Bundle every icon so the static site never asks the Iconify API at runtime.
    clientBundle: {
      scan: true,
      icons: ['simple-icons:rust', 'simple-icons:toml', 'simple-icons:typescript', 'simple-icons:json', 'simple-icons:npm', 'lucide:terminal',
        // .navigation.yml icons are not found by the scanner.
        'lucide:rocket', 'lucide:monitor-smartphone', 'lucide:file-code', 'lucide:book-open'],
      sizeLimitKb: 512,
    },
  },

  colorMode: {
    preference: 'system',
  },

  content: {
    build: {
      markdown: {
        toc: { depth: 3, searchDepth: 2 },
        highlight: {
          langs: ['rust', 'toml', 'bash', 'shell', 'ts', 'js', 'json', 'jsonc', 'yaml', 'diff', 'vue'],
        },
      },
    },
    experimental: {
      // node:sqlite (Node 22.5+): no native better-sqlite3 build.
      sqliteConnector: 'native',
    },
  },

  nitro: {
    prerender: {
      // `/guides/parsers.html`, not `/guides/parsers/index.html`: GitHub Pages serves it
      // without a redirect to the trailing-slash URL.
      autoSubfolderIndex: false,
      crawlLinks: true,
      failOnError: true,
      routes: ['/'],
    },
  },

  compatibilityDate: '2025-11-03',
})
