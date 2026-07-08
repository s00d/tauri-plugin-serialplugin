<script setup lang="ts">
import { onMounted, ref } from 'vue';
import { invoke } from '@tauri-apps/api/core';
import { SerialPort, LogLevel, type Capabilities, type LogLevelType } from 'tauri-plugin-serialplugin-api';

const caps = ref<Capabilities | null>(null);
const logLevel = ref<LogLevelType>('Info');
const rustOutput = ref<string | null>(null);
const busy = ref(false);

const logLevels = Object.values(LogLevel);

onMounted(async () => {
  try {
    caps.value = await SerialPort.getCapabilities();
    logLevel.value = await SerialPort.getLogLevel();
  } catch (e) {
    console.error(e);
  }
});

async function setLog(level: LogLevelType) {
  logLevel.value = level;
  await SerialPort.setLogLevel(level);
}

async function runRustPorts() {
  busy.value = true;
  rustOutput.value = null;
  try {
    rustOutput.value = await invoke<string>('get_ports_programmatically');
  } catch (e) {
    rustOutput.value = `Error: ${String(e)}`;
  } finally {
    busy.value = false;
  }
}
</script>

<template>
  <header class="bar panel">
    <div class="brand">
      <strong>serialplugin</strong>
      <span class="muted">v3 demo</span>
    </div>

    <div v-if="caps" class="caps">
      <span class="badge data">{{ caps.transport }}</span>
      <span class="badge off">{{ caps.platform }}</span>
      <span class="badge off">v{{ caps.version }}</span>
    </div>

    <div class="actions">
      <label class="log-level">
        Log
        <select :value="logLevel" @change="setLog(($event.target as HTMLSelectElement).value as LogLevelType)">
          <option v-for="lvl in logLevels" :key="lvl" :value="lvl">{{ lvl }}</option>
        </select>
      </label>
      <button
        v-if="caps?.transport === 'desktop'"
        type="button"
        class="ghost"
        :disabled="busy"
        @click="runRustPorts"
      >
        Rust ports
      </button>
    </div>

    <pre v-if="rustOutput" class="rust-out">{{ rustOutput }}</pre>
  </header>
</template>

<style scoped>
.bar {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: var(--gap);
  padding: 8px 12px;
  margin-bottom: var(--gap);
}

@media (max-width: 768px) {
  .bar {
    padding: 6px 8px;
    margin-bottom: 6px;
  }

  .actions {
    width: 100%;
    justify-content: flex-end;
  }

  .rust-out {
    max-height: 80px;
  }
}

.brand {
  display: flex;
  align-items: baseline;
  gap: 8px;
}

.brand strong {
  font-size: 0.9375rem;
}

.muted {
  color: var(--muted);
  font-size: 0.75rem;
}

.caps {
  display: flex;
  gap: 6px;
  flex-wrap: wrap;
}

.actions {
  display: flex;
  align-items: center;
  gap: 8px;
  margin-left: auto;
}

.log-level {
  flex-direction: row;
  align-items: center;
  gap: 6px;
}

.log-level select {
  width: auto;
  min-width: 5.5rem;
}

.rust-out {
  flex: 1 1 100%;
  margin: 0;
  padding: 8px;
  background: var(--bg);
  border: 1px solid var(--border);
  border-radius: 6px;
  font-family: var(--font-mono);
  font-size: 0.6875rem;
  max-height: 120px;
  overflow: auto;
  white-space: pre-wrap;
}
</style>
