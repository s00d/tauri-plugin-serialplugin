<script setup lang="ts">
import type { ButtonProps, PageFeatureProps } from '@nuxt/ui'

const { data: page } = await useAsyncData('landing', () => queryCollection('landing').path('/').first())

useSeoMeta({
  title: 'tauri-plugin-serialplugin',
  description: page.value?.description,
  ogTitle: 'tauri-plugin-serialplugin',
  ogDescription: page.value?.description,
})

const heroLinks: ButtonProps[] = [
  { label: 'Get started', to: '/getting-started/introduction', size: 'lg', trailingIcon: 'i-lucide-arrow-right' },
  { label: 'GitHub', to: 'https://github.com/s00d/tauri-plugin-serialplugin', target: '_blank', size: 'lg', color: 'neutral', variant: 'outline', icon: 'i-simple-icons-github' },
]

const features: PageFeatureProps[] = [
  { title: 'One API everywhere', description: 'Open, stream with watch(), read/write, line settings, modem lines, hotplug, auto-reconnect.', icon: 'i-lucide-cable', to: '/guides/javascript-api' },
  { title: 'Modems built in', description: 'A native AT command queue with structured results, URCs, SMS (PDU) and GSM 07.10 / 27.010 CMUX.', icon: 'i-lucide-radio-tower', to: '/guides/at-commands' },
  { title: 'Android without Java libraries', description: 'USB-serial drivers in Rust (FTDI, CP210x, PL2303, CH34x, CDC-ACM, …).', icon: 'i-lucide-smartphone', to: '/platforms/android' },
  { title: 'Network and radio transports', description: 'tcp://host:port to a serial bridge, ble://<id> to Nordic UART devices.', icon: 'i-lucide-wifi', to: '/platforms/tcp' },
  { title: 'Rust backend', description: 'The same operations from your Tauri backend through the managed SerialPort state.', icon: 'i-simple-icons-rust', to: '/guides/rust' },
  { title: 'Generated reference', description: 'Every method, type, Tauri command and permission, built from the sources on each release.', icon: 'i-lucide-book-open', to: '/reference/serial-port' },
]

const quickLinks: ButtonProps[] = [
  { label: 'Quick start', to: '/getting-started/quick-start', trailingIcon: 'i-lucide-arrow-right' },
  { label: 'Upgrading from v3', to: '/migration', color: 'neutral', variant: 'outline' },
]
</script>

<template>
  <div>
    <UPageHero
      title="Serial ports for Tauri 2 apps"
      description="Desktop COM/tty ports, USB-serial adapters on Android, and serial over TCP or Bluetooth LE — including on iOS."
      :links="heroLinks"
    >
      <template #headline>
        <UButton to="/migration" label="v4 is out: what changed" trailing-icon="i-lucide-arrow-right" color="neutral" variant="subtle" size="sm" />
      </template>
    </UPageHero>

    <UPageSection title="What you get" :features="features" />

    <UPageSection
      title="Open a port, watch it, write to it"
      description="List the ports, open one and stream incoming data with watch(). Binary data, AT commands and auto-reconnect use the same port object."
      orientation="horizontal"
      :links="quickLinks"
    >
      <div class="min-w-0">
        <ContentRenderer v-if="page" :value="page" />
      </div>
    </UPageSection>
  </div>
</template>
