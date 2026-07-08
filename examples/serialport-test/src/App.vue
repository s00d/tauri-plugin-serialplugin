<script setup lang="ts">
import { ref } from 'vue';
import StatusBar from './components/StatusBar.vue';
import PortPicker from './components/PortPicker.vue';
import PortWorkspace from './components/PortWorkspace.vue';

const selectedPort = ref<string | null>(null);
const portsOpen = ref(false);

function selectPort(path: string) {
  selectedPort.value = path;
  portsOpen.value = false;
}

function clearPort() {
  selectedPort.value = null;
}
</script>

<template>
  <div class="app">
    <StatusBar />

    <div class="main">
      <PortPicker
        class="sidebar"
        :class="{ open: portsOpen }"
        :selected="selectedPort"
        @select="selectPort"
        @close="portsOpen = false"
      />

      <div v-if="portsOpen" class="backdrop" @click="portsOpen = false" />

      <div class="content">
        <div v-if="!selectedPort" class="mobile-bar panel">
          <button type="button" class="primary" @click="portsOpen = true">Ports</button>
          <p class="hint">Select a serial port to open the terminal</p>
        </div>

        <PortWorkspace
          v-if="selectedPort"
          :key="selectedPort"
          :port-path="selectedPort"
          @back="clearPort"
          @ports="portsOpen = true"
        />

        <div v-else class="placeholder panel desktop-only">
          <p>Select a port from the list or enter a path manually.</p>
          <p class="sub">Terminal at the bottom — watch, send text/AT/hex, signals &amp; config in Tools.</p>
        </div>
      </div>
    </div>
  </div>
</template>

<style scoped>
.app {
  max-width: 1400px;
  margin: 0 auto;
  padding: 8px;
  min-height: 100dvh;
  display: flex;
  flex-direction: column;
}

.main {
  display: grid;
  grid-template-columns: 260px 1fr;
  gap: var(--gap);
  flex: 1;
  min-height: 0;
  height: calc(100dvh - 72px);
  position: relative;
}

.content {
  display: flex;
  flex-direction: column;
  min-height: 0;
  min-width: 0;
}

.mobile-bar {
  display: none;
  align-items: center;
  gap: 10px;
  padding: 10px 12px;
  margin-bottom: var(--gap);
}

.mobile-bar .hint {
  margin: 0;
  font-size: 0.8125rem;
  color: var(--muted);
}

.placeholder {
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  text-align: center;
  padding: 2rem;
  color: var(--muted);
  flex: 1;
}

.placeholder p {
  margin: 0 0 8px;
}

.sub {
  font-size: 0.8125rem;
  max-width: 360px;
}

.backdrop {
  display: none;
}

@media (max-width: 768px) {
  .app {
    padding: 6px;
    padding-bottom: env(safe-area-inset-bottom, 6px);
  }

  .main {
    grid-template-columns: 1fr;
    height: calc(100dvh - 64px);
  }

  .sidebar {
    position: fixed;
    top: 0;
    left: 0;
    bottom: 0;
    width: min(300px, 88vw);
    z-index: 40;
    transform: translateX(-105%);
    transition: transform 0.22s ease;
    border-radius: 0;
    box-shadow: none;
  }

  .sidebar.open {
    transform: translateX(0);
    box-shadow: 8px 0 24px rgba(0, 0, 0, 0.45);
  }

  .backdrop {
    display: block;
    position: fixed;
    inset: 0;
    z-index: 30;
    background: rgba(0, 0, 0, 0.55);
  }

  .mobile-bar {
    display: flex;
  }

  .desktop-only {
    display: none;
  }
}
</style>
