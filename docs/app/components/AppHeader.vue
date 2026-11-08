<script setup lang="ts">
import type { ContentNavigationItem } from '@nuxt/content'

const { nav, github } = useAppConfig()
const route = useRoute()
const navigation = inject<Ref<ContentNavigationItem[]>>('navigation')

const items = computed(() =>
  nav.map((item) => {
    const section = item.to.split('/').slice(0, 2).join('/')
    return { ...item, active: route.path.startsWith(section) }
  }),
)
</script>

<template>
  <UHeader>
    <template #title>
      <img src="/logo.svg" alt="" width="24" height="24" class="size-6 shrink-0" />
      <span class="font-semibold">serialplugin</span>
    </template>

    <UNavigationMenu :items="items" variant="link" />

    <template #right>
      <UContentSearchButton class="lg:hidden" />
      <UContentSearchButton :collapsed="false" class="hidden lg:flex" />
      <UColorModeButton />
      <UButton
        icon="i-simple-icons-npm"
        color="neutral"
        variant="ghost"
        square
        to="https://www.npmjs.com/package/tauri-plugin-serialplugin-api"
        target="_blank"
        aria-label="tauri-plugin-serialplugin-api on npm"
        class="hidden sm:inline-flex"
      />
      <UButton
        icon="i-simple-icons-rust"
        color="neutral"
        variant="ghost"
        square
        to="https://crates.io/crates/tauri-plugin-serialplugin"
        target="_blank"
        aria-label="tauri-plugin-serialplugin on crates.io"
        class="hidden sm:inline-flex"
      />
      <UButton icon="i-simple-icons-github" color="neutral" variant="ghost" square :to="github" target="_blank" aria-label="tauri-plugin-serialplugin on GitHub" />
    </template>

    <template #body>
      <UNavigationMenu :items="items" orientation="vertical" class="mb-4" />
      <UContentNavigation highlight :navigation="navigation" />
    </template>
  </UHeader>
</template>
