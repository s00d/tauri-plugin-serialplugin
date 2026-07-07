<script setup lang="ts">
import { ref, toRef, watch } from 'vue';
import { usePortSession } from '../composables/usePortSession';
import PortSettings from './PortSettings.vue';
import PortSignals from './PortSignals.vue';
import PortAtConsole from './PortAtConsole.vue';
import TerminalPanel from './TerminalPanel.vue';
import type { LineEnding, SendMode } from '../types';

const props = defineProps<{
  portPath: string;
}>();

const emit = defineEmits<{
  back: [];
  ports: [];
}>();

const pathRef = toRef(props, 'portPath');
const {
  connected,
  watching,
  channelId,
  lastError,
  terminalLines,
  bytesToRead,
  bytesToWrite,
  baudRate,
  dataBits,
  flowControl,
  parity,
  stopBits,
  timeout,
  flushMs,
  autoReconnect,
  autoReconnectInfo,
  rts,
  dtr,
  cts,
  dsr,
  ri,
  cd,
  connect,
  disconnect,
  applySettings,
  sendPayload,
  sendBinaryDemo,
  pollRead,
  clearBuffers,
  toggleRts,
  toggleDtr,
  refreshSignals,
  clearTerminal,
  resetForPath,
  atBusy,
  atEntries,
  cancelAt,
  runAtScript,
} = usePortSession(pathRef);

const toolsOpen = ref(false);
const toolsTab = ref<'signals' | 'at' | 'config'>('signals');

watch(
  () => props.portPath,
  () => {
    resetForPath();
    toolsOpen.value = false;
    toolsTab.value = 'signals';
  },
);

function onSend(payload: { text: string; mode: SendMode; lineEnding: LineEnding; localEcho: boolean }) {
  void sendPayload(payload.text, {
    mode: payload.mode,
    lineEnding: payload.lineEnding,
    localEcho: payload.localEcho,
  });
}
</script>

<template>
  <section class="workspace">
    <header class="ws-head panel">
      <div class="title-block">
        <button type="button" class="ghost back-btn" aria-label="Back to ports" @click="emit('back')">
          ←
        </button>
        <button type="button" class="ghost ports-btn" aria-label="Port list" @click="emit('ports')">
          ☰
        </button>
        <div class="title-row">
          <code class="path">{{ portPath }}</code>
          <span v-if="connected" class="badge ok">open</span>
          <span v-else class="badge off">closed</span>
          <span v-if="watching" class="badge data">watch #{{ channelId }}</span>
          <span
            v-if="autoReconnectInfo?.enabled"
            class="badge off"
            :title="`attempts: ${autoReconnectInfo.currentAttempts}`"
          >
            auto-rc
          </span>
        </div>
      </div>

      <div class="conn-actions">
        <button type="button" class="primary" :disabled="connected" @click="connect">Connect</button>
        <button type="button" class="danger" :disabled="!connected" @click="disconnect">Disconnect</button>
        <button
          type="button"
          class="ghost tools-toggle"
          :class="{ active: toolsOpen }"
          @click="toolsOpen = !toolsOpen"
        >
          Tools
        </button>
      </div>
    </header>

    <div class="terminal-wrap">
      <TerminalPanel
        :connected="connected"
        :watching="watching"
        :lines="terminalLines"
        :at-busy="atBusy"
        @send="onSend"
        @clear="clearTerminal"
        @poll-read="pollRead"
        @binary="sendBinaryDemo"
        @clear-buf="clearBuffers"
      />
    </div>

    <Transition name="slide">
      <div v-if="toolsOpen" class="tools panel">
        <div class="tools-tabs">
          <button type="button" :class="{ active: toolsTab === 'signals' }" @click="toolsTab = 'signals'">
            Signals
          </button>
          <button type="button" :class="{ active: toolsTab === 'at' }" @click="toolsTab = 'at'">AT queue</button>
          <button type="button" :class="{ active: toolsTab === 'config' }" @click="toolsTab = 'config'">Config</button>
        </div>

        <div v-show="toolsTab === 'signals'" class="tools-body">
          <PortSignals
            :connected="connected"
            :watching="watching"
            :rts="rts"
            :dtr="dtr"
            :cts="cts"
            :dsr="dsr"
            :ri="ri"
            :cd="cd"
            :bytes-to-read="bytesToRead"
            :bytes-to-write="bytesToWrite"
            @toggle-rts="toggleRts"
            @toggle-dtr="toggleDtr"
            @refresh="refreshSignals"
          />
        </div>

        <div v-show="toolsTab === 'at'" class="tools-body">
          <PortAtConsole
            :connected="connected"
            :watching="watching"
            :at-busy="atBusy"
            :entries="atEntries"
            @cancel="cancelAt"
            @run-script="runAtScript"
          />
        </div>

        <div v-show="toolsTab === 'config'" class="tools-body">
          <PortSettings
            :connected="connected"
            :baud-rate="baudRate"
            :data-bits="dataBits"
            :flow-control="flowControl"
            :parity="parity"
            :stop-bits="stopBits"
            :timeout="timeout"
            :flush-ms="flushMs"
            :auto-reconnect="autoReconnect"
            @update:baud-rate="baudRate = $event"
            @update:data-bits="dataBits = $event"
            @update:flow-control="flowControl = $event"
            @update:parity="parity = $event"
            @update:stop-bits="stopBits = $event"
            @update:timeout="timeout = $event"
            @update:flush-ms="flushMs = $event"
            @update:auto-reconnect="autoReconnect = $event"
            @apply="applySettings"
          />
          <p v-if="lastError" class="err">{{ lastError }}</p>
        </div>
      </div>
    </Transition>
  </section>
</template>

<style scoped>
.workspace {
  display: flex;
  flex-direction: column;
  min-height: 0;
  flex: 1;
  gap: var(--gap);
}

.ws-head {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  justify-content: space-between;
  gap: 8px;
  padding: 8px 10px;
}

.title-block {
  display: flex;
  align-items: center;
  gap: 6px;
  min-width: 0;
}

.back-btn {
  display: none;
  padding: 4px 8px;
  font-size: 1rem;
}

.ports-btn {
  display: none;
  padding: 4px 8px;
  font-size: 0.875rem;
}

.title-row {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 6px;
  min-width: 0;
}

.path {
  font-size: 0.75rem;
  color: var(--text);
  word-break: break-all;
}

.conn-actions {
  display: flex;
  gap: 6px;
  flex-wrap: wrap;
}

.tools-toggle.active {
  border-color: var(--accent);
  color: var(--text);
}

.terminal-wrap {
  display: flex;
  flex-direction: column;
  min-height: 0;
  flex: 1;
}

.tools {
  display: flex;
  flex-direction: column;
  max-height: 40vh;
  min-height: 0;
  overflow: hidden;
}

.tools-tabs {
  display: flex;
  gap: 4px;
  padding: 6px 8px;
  border-bottom: 1px solid var(--border);
  flex-shrink: 0;
}

.tools-tabs button {
  background: transparent;
  border-color: transparent;
  color: var(--muted);
  font-size: 0.75rem;
  padding: 4px 10px;
}

.tools-tabs button.active {
  color: var(--text);
  border-color: var(--border);
  background: var(--surface-2);
}

.tools-body {
  padding: 10px;
  overflow-y: auto;
  flex: 1;
  min-height: 0;
  -webkit-overflow-scrolling: touch;
}

.err {
  margin: 8px 0 0;
  padding: 8px;
  border-radius: 6px;
  background: color-mix(in srgb, var(--err) 15%, transparent);
  color: #fca5a5;
  font-size: 0.75rem;
  font-family: var(--font-mono);
}

.slide-enter-active,
.slide-leave-active {
  transition: max-height 0.2s ease, opacity 0.2s ease;
  overflow: hidden;
}

.slide-enter-from,
.slide-leave-to {
  max-height: 0;
  opacity: 0;
}

.slide-enter-to,
.slide-leave-from {
  max-height: 40vh;
  opacity: 1;
}

@media (max-width: 768px) {
  .back-btn,
  .ports-btn {
    display: inline-flex;
  }

  .tools {
    max-height: 50vh;
  }
}
</style>
