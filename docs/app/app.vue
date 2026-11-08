<script setup lang="ts">
const siteName = 'tauri-plugin-serialplugin'

const { data: navigation } = await useAsyncData('navigation', () => queryCollectionNavigation('docs'), { default: () => [] })
// Loaded after hydration: the search index is in no page payload.
const { data: files } = useLazyAsyncData('search', () => queryCollectionSearchSections('docs'), { server: false })

provide('navigation', navigation)

useHead({
  meta: [{ name: 'viewport', content: 'width=device-width, initial-scale=1' }],
  titleTemplate: (title?: string) => (!title || title === siteName ? siteName : `${title} · ${siteName}`),
})

useSeoMeta({
  ogSiteName: siteName,
  ogType: 'website',
})
</script>

<template>
  <UApp>
    <NuxtLoadingIndicator color="var(--ui-primary)" />
    <AppHeader />
    <UMain>
      <NuxtLayout>
        <NuxtPage />
      </NuxtLayout>
    </UMain>
    <ClientOnly>
      <LazyUContentSearch :files="files" :navigation="navigation" />
    </ClientOnly>
  </UApp>
</template>
