<script setup lang="ts">
import type { NuxtError } from '#app'

const props = defineProps<{ error: NuxtError }>()

const { data: navigation } = await useAsyncData('navigation', () => queryCollectionNavigation('docs'), { default: () => [] })
provide('navigation', navigation)

const notFound = computed(() => props.error.statusCode === 404)
useSeoMeta({ title: () => (notFound.value ? 'Page not found' : 'Error'), robots: 'noindex' })
</script>

<template>
  <UApp>
    <AppHeader />
    <UMain>
      <UContainer>
        <UPageHero
          :headline="String(error.statusCode || 500)"
          :title="notFound ? 'Page not found' : 'Something went wrong'"
          :description="notFound ? 'This page does not exist or has moved.' : error.message"
          :links="[{ label: 'Home', to: '/', trailingIcon: 'i-lucide-arrow-right' }]"
        />
      </UContainer>
    </UMain>
  </UApp>
</template>
