<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref } from 'vue';
import { SerialPort } from 'tauri-plugin-serialplugin-api';
import type { PortInfo, WatchHandle } from 'tauri-plugin-serialplugin-api';
import type { PortListEntry } from '../types';

const props = defineProps<{
  selected: string | null;
}>();

const emit = defineEmits<{
  select: [path: string];
  close: [];
}>();

const manualPath = ref('');
const available = ref<Record<string, { type: string }>>({});
const managed = ref<string[]>([]);
const loading = ref(false);
const watching = ref(false);
const lastEvent = ref<string | null>(null);
let portsWatch: WatchHandle | null = null;

const entries = computed<PortListEntry[]>(() => {
  const map = new Map<string, PortListEntry>();

  for (const [path, info] of Object.entries(available.value)) {
    map.set(path, { path, meta: info.type, source: 'available' });
  }
  for (const path of managed.value) {
    if (!map.has(path)) {
      map.set(path, { path, meta: 'managed', source: 'managed' });
    }
  }

  return [...map.values()].sort((a, b) => a.path.localeCompare(b.path));
});

function applySnapshot(ports: Record<string, PortInfo>) {
  available.value = Object.fromEntries(
    Object.entries(ports).map(([path, info]) => [path, { type: info.type ?? 'Unknown' }]),
  );
}

async function refresh() {
  loading.value = true;
  try {
    const [ports, open] = await Promise.all([
      SerialPort.available_ports({ singlePortPerDevice: true }),
      SerialPort.managed_ports(),
    ]);
    applySnapshot(ports);
    managed.value = open;
  } catch (e) {
    console.error(e);
  } finally {
    loading.value = false;
  }
}

async function startPortWatch() {
  if (portsWatch) return;
  try {
    portsWatch = await SerialPort.watchAvailablePorts(
      {
        onSnapshot: (ports) => {
          applySnapshot(ports);
          lastEvent.value = `snapshot (${Object.keys(ports).length} ports)`;
        },
        onAdded: (path, info) => {
          available.value = {
            ...available.value,
            [path]: { type: info.type ?? 'Unknown' },
          };
          lastEvent.value = `+ ${path}`;
        },
        onRemoved: (path) => {
          const next = { ...available.value };
          delete next[path];
          available.value = next;
          lastEvent.value = `− ${path}`;
        },
      },
      { singlePortPerDevice: true, pollIntervalMs: 2000 },
    );
    watching.value = true;
  } catch (e) {
    console.error('port watch failed', e);
    watching.value = false;
  }
}

async function stopPortWatch() {
  if (portsWatch) {
    await portsWatch.unwatch();
    portsWatch = null;
  }
  watching.value = false;
}

function addManual() {
  const path = manualPath.value.trim();
  if (!path) return;
  emit('select', path);
  manualPath.value = '';
}

onMounted(async () => {
  await refresh();
  await startPortWatch();
});

onUnmounted(() => {
  void stopPortWatch();
});
</script>

<template>
  <aside class="picker panel">
    <div class="head">
      <h2>Ports</h2>
      <div class="head-actions">
        <span v-if="watching" class="live" title="Hotplug subscription active">● live</span>
        <button type="button" class="ghost" :disabled="loading" @click="refresh">
          {{ loading ? '…' : '↻' }}
        </button>
        <button type="button" class="ghost mobile-close" aria-label="Close" @click="emit('close')">✕</button>
      </div>
    </div>

    <p v-if="lastEvent" class="event-hint">{{ lastEvent }}</p>

    <div class="manual">
      <input
        v-model="manualPath"
        type="text"
        placeholder="COM3 / /dev/ttyUSB0"
        @keyup.enter="addManual"
      />
      <button type="button" class="primary" @click="addManual">Open</button>
    </div>

    <ul v-if="entries.length" class="list">
      <li v-for="item in entries" :key="item.path">
        <button
          type="button"
          class="port-btn"
          :class="{ active: selected === item.path }"
          @click="emit('select', item.path)"
        >
          <span class="path">{{ item.path }}</span>
          <span class="meta">{{ item.meta }}</span>
        </button>
      </li>
    </ul>
    <p v-else class="empty">No ports — plug a device</p>
  </aside>
</template>

<style scoped>
.picker {
  display: flex;
  flex-direction: column;
  min-height: 0;
  overflow: hidden;
}

.head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 10px 12px;
  border-bottom: 1px solid var(--border);
}

.head-actions {
  display: flex;
  align-items: center;
  gap: 8px;
}

.live {
  font-size: 0.6875rem;
  color: var(--ok, #3dd68c);
  letter-spacing: 0.02em;
}

.event-hint {
  margin: 0;
  padding: 6px 12px;
  font-size: 0.6875rem;
  font-family: var(--font-mono);
  color: var(--muted);
  border-bottom: 1px solid var(--border);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.head h2 {
  margin: 0;
  font-size: 0.8125rem;
  font-weight: 600;
  text-transform: uppercase;
  letter-spacing: 0.04em;
  color: var(--muted);
}

.manual {
  display: grid;
  grid-template-columns: 1fr auto;
  gap: 6px;
  padding: 10px 12px;
  border-bottom: 1px solid var(--border);
}

.list {
  list-style: none;
  margin: 0;
  padding: 6px;
  overflow-y: auto;
  flex: 1;
}

.port-btn {
  width: 100%;
  text-align: left;
  display: flex;
  flex-direction: column;
  gap: 2px;
  padding: 8px 10px;
  margin-bottom: 4px;
  background: transparent;
  border-color: transparent;
}

.port-btn:hover {
  background: var(--surface-2);
}

.port-btn.active {
  border-color: var(--accent);
  background: color-mix(in srgb, var(--accent) 12%, var(--surface));
}

.path {
  font-family: var(--font-mono);
  font-size: 0.75rem;
  word-break: break-all;
}

.meta {
  font-size: 0.6875rem;
  color: var(--muted);
}

.empty {
  margin: 0;
  padding: 16px 12px;
  color: var(--muted);
  font-size: 0.8125rem;
  text-align: center;
}

.mobile-close {
  display: none;
}

@media (max-width: 768px) {
  .mobile-close {
    display: inline-flex;
  }
}
</style>
