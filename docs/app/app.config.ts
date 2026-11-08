export default defineAppConfig({
  ui: {
    colors: {
      primary: 'orange',
      neutral: 'zinc',
    },
    contentNavigation: {
      slots: {
        // Wrap long titles instead of truncating them.
        linkTitle: 'text-clip whitespace-normal text-pretty text-start',
      },
    },
    contentToc: {
      slots: {
        linkText: 'text-clip whitespace-normal text-pretty',
      },
    },
    prose: {
      codeIcon: {
        rust: 'i-simple-icons-rust',
        rs: 'i-simple-icons-rust',
        toml: 'i-simple-icons-toml',
        'Cargo.toml': 'i-simple-icons-rust',
        ts: 'i-simple-icons-typescript',
        jsonc: 'i-simple-icons-json',
        json: 'i-simple-icons-json',
        bash: 'i-lucide-terminal',
        sh: 'i-lucide-terminal',
      },
      code: {
        base: 'inline px-1 py-px text-[0.875em] font-medium rounded box-decoration-clone wrap-break-word',
        variants: {
          color: {
            neutral: 'border-0 bg-elevated text-highlighted',
          },
        },
      },
      a: { base: '[&>code]:text-primary' },
      h2: { slots: { base: 'mt-12 mb-4' } },
      h3: { slots: { base: 'mt-8 mb-2' } },
      table: {
        slots: {
          root: 'my-6 rounded-lg border border-default',
          base: 'border-collapse',
        },
      },
      thead: { base: 'bg-elevated/60' },
      th: {
        base: 'py-2 px-3 align-bottom text-highlighted border-0 border-b first:border-s-0 border-default whitespace-nowrap',
      },
      td: {
        base: 'py-2 px-3 align-top leading-6 border-0 border-b first:border-s-0 border-default [&_code]:text-[0.8125rem] [&_code]:leading-6',
      },
      tr: { base: '[&:last-child>td]:border-b-0' },
    },
  },
  github: 'https://github.com/s00d/tauri-plugin-serialplugin',
  edit: 'https://github.com/s00d/tauri-plugin-serialplugin/edit/main/docs/content',
  nav: [
    { label: 'Guide', to: '/getting-started/introduction' },
    { label: 'Guides', to: '/guides/javascript-api' },
    { label: 'API reference', to: '/reference/serial-port' },
    { label: 'Platforms', to: '/platforms/overview' },
    { label: 'Changelog', to: '/changelog' },
  ],
})
