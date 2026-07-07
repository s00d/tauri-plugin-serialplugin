<script setup lang="ts">
import { ref } from 'vue';
import type { AtParseStatus } from 'tauri-plugin-serialplugin-api';

export type AtEntryStatus = 'running' | 'done' | 'error';

export interface AtEntry {
  id: number;
  command: string;
  status: AtEntryStatus;
  parseStatus?: AtParseStatus;
  response?: string;
  urcLines?: string[];
  error?: string;
}

defineProps<{
  connected: boolean;
  watching: boolean;
  atBusy: boolean;
  entries: AtEntry[];
}>();

const emit = defineEmits<{
  cancel: [];
  runScript: [lines: string[]];
}>();

const script = ref('AT\nATI\nAT+CGMI');

function runScript() {
  const lines = script.value
    .split('\n')
    .map((l) => l.trim())
    .filter(Boolean);
  if (lines.length === 0) return;
  emit('runScript', lines);
}
</script>

<template>
  <div class="at-console">
    <p class="hint">
      Single AT commands: use terminal mode <strong>AT</strong>. Batch script uses one native
      <code>sendAtPhases()</code> job (250&nbsp;ms gap between lines on Android / CH340).
    </p>

    <div class="toolbar">
      <button type="button" :disabled="!connected || !atBusy" @click="emit('cancel')">
        Cancel
      </button>
      <span v-if="atBusy" class="badge ok">running</span>
      <span v-if="watching" class="badge data">watch active</span>
    </div>

    <label class="script-label">
      Script (one command per line)
      <textarea v-model="script" rows="3" :disabled="!connected" />
    </label>
    <button
      type="button"
      class="primary run-script"
      :class="{ loading: atBusy }"
      :disabled="!connected || atBusy"
      @click="runScript"
    >
      <span v-if="atBusy" class="spinner" aria-hidden="true" />
      {{ atBusy ? 'Running…' : 'Run script' }}
    </button>

    <div v-if="entries.length" class="queue-wrap">
      <table class="queue-table">
        <thead>
          <tr>
            <th>Command</th>
            <th>Status</th>
            <th>Parse</th>
            <th>Response</th>
          </tr>
        </thead>
        <tbody>
          <tr v-for="entry in entries" :key="entry.id" :class="entry.status">
            <td><code>{{ entry.command }}</code></td>
            <td>{{ entry.status }}</td>
            <td>
              <span v-if="entry.parseStatus" class="parse" :class="entry.parseStatus">{{ entry.parseStatus }}</span>
              <span v-else>—</span>
            </td>
            <td class="response">
              <span v-if="entry.response">{{ entry.response.slice(0, 80) }}</span>
              <span v-else-if="entry.error" class="err">{{ entry.error }}</span>
              <span v-else>—</span>
            </td>
          </tr>
        </tbody>
      </table>
    </div>
  </div>
</template>

<style scoped>
.at-console {
  display: flex;
  flex-direction: column;
  gap: 0.65rem;
}

.hint {
  font-size: 0.75rem;
  color: var(--muted);
  margin: 0;
  line-height: 1.45;
}

.toolbar {
  display: flex;
  gap: 0.5rem;
  align-items: center;
  flex-wrap: wrap;
}

.script-label {
  display: flex;
  flex-direction: column;
  gap: 0.25rem;
  font-size: 0.75rem;
}

.queue-wrap {
  overflow-x: auto;
  -webkit-overflow-scrolling: touch;
}

.queue-table {
  width: 100%;
  border-collapse: collapse;
  font-size: 0.75rem;
  font-family: var(--font-mono);
}

.queue-table th,
.queue-table td {
  border: 1px solid var(--border);
  padding: 0.35rem 0.5rem;
  text-align: left;
  vertical-align: top;
}

.queue-table th {
  background: var(--surface-2);
  color: var(--muted);
  font-size: 0.6875rem;
  text-transform: uppercase;
}

.parse.ok {
  color: #86efac;
}

.parse.error,
.parse.cme,
.parse.cms {
  color: #fca5a5;
}

.err {
  color: #fca5a5;
}

.run-script {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  justify-content: center;
  min-width: 7rem;
}

.run-script.loading {
  opacity: 0.85;
}

.spinner {
  width: 0.875rem;
  height: 0.875rem;
  border: 2px solid rgba(255, 255, 255, 0.35);
  border-top-color: #fff;
  border-radius: 50%;
  animation: spin 0.7s linear infinite;
}

@keyframes spin {
  to {
    transform: rotate(360deg);
  }
}
</style>
