<script setup lang="ts">
import { nextTick, onMounted, ref, watch } from 'vue';
import type { LineEnding, SendMode, TerminalLine } from '../types';

const props = defineProps<{
  connected: boolean;
  watching: boolean;
  lines: TerminalLine[];
  atBusy?: boolean;
}>();

const emit = defineEmits<{
  send: [payload: { text: string; mode: SendMode; lineEnding: LineEnding; localEcho: boolean }];
  clear: [];
  pollRead: [];
  binary: [];
  clearBuf: [];
}>();

const input = ref('');
const mode = ref<SendMode>('text');
const lineEnding = ref<LineEnding>('lf');
const localEcho = ref(true);
const autoScroll = ref(true);
const scrollEl = ref<HTMLElement | null>(null);

const modeLabels: Record<SendMode, string> = {
  text: 'Text',
  at: 'AT',
  hex: 'Hex',
};

function send() {
  const text = input.value;
  if (!text.trim() || !props.connected) return;
  emit('send', {
    text,
    mode: mode.value,
    lineEnding: lineEnding.value,
    localEcho: localEcho.value,
  });
  input.value = '';
}

function scrollToBottom() {
  if (!autoScroll.value || !scrollEl.value) return;
  scrollEl.value.scrollTop = scrollEl.value.scrollHeight;
}

watch(
  () => props.lines.length,
  () => {
    void nextTick(scrollToBottom);
  },
);

onMounted(scrollToBottom);
</script>

<template>
  <div class="terminal">
    <div ref="scrollEl" class="output">
      <div v-if="!lines.length" class="welcome">
        <p>Serial terminal</p>
        <ul>
          <li>Connect the port, then type below and press Enter</li>
          <li><strong>Text</strong> — raw write; <strong>AT</strong> — <code>sendAt()</code>; <strong>Hex</strong> — binary</li>
          <li>RX / URC / system messages stream here via <code>watch()</code></li>
        </ul>
      </div>
      <div
        v-for="line in lines"
        :key="line.id"
        class="line"
        :class="line.kind"
      >
        <span class="time">{{ line.time }}</span>
        <span class="tag">{{ line.kind }}</span>
        <span class="text">{{ line.text }}</span>
      </div>
    </div>

    <div class="toolbar">
      <label class="chk">
        <input v-model="autoScroll" type="checkbox" />
        scroll
      </label>
      <label class="chk">
        <input v-model="localEcho" type="checkbox" />
        echo TX
      </label>
      <button type="button" class="ghost" @click="emit('clear')">Clear</button>
      <button type="button" class="ghost" :disabled="!connected || watching" @click="emit('pollRead')">
        Poll read
      </button>
      <button type="button" class="ghost" :disabled="!connected" @click="emit('binary')">Hi\\n</button>
      <button type="button" class="ghost" :disabled="!connected" @click="emit('clearBuf')">Flush buf</button>
    </div>

    <div class="dock">
      <div class="mode-row">
        <div class="modes">
          <button
            v-for="(label, key) in modeLabels"
            :key="key"
            type="button"
            :class="{ active: mode === key }"
            @click="mode = key as SendMode"
          >
            {{ label }}
          </button>
        </div>
        <select v-if="mode === 'text'" v-model="lineEnding" class="ending" aria-label="Line ending">
          <option value="lf">LF</option>
          <option value="cr">CR</option>
          <option value="crlf">CRLF</option>
          <option value="none">none</option>
        </select>
      </div>

      <div class="input-row">
        <span class="prompt">&gt;</span>
        <input
          v-model="input"
          type="text"
          :placeholder="
            mode === 'hex'
              ? '48 65 6c 6c 6f'
              : mode === 'at'
                ? 'AT+CGMI'
                : 'Type message…'
          "
          :disabled="!connected"
          autocomplete="off"
          autocapitalize="off"
          spellcheck="false"
          @keyup.enter="send"
        />
        <button
          type="button"
          class="primary send-btn"
          :class="{ loading: mode === 'at' && atBusy }"
          :disabled="!connected || !input.trim()"
          @click="send"
        >
          <span v-if="mode === 'at' && atBusy" class="spinner" aria-hidden="true" />
          Send
        </button>
      </div>
    </div>
  </div>
</template>

<style scoped>
.terminal {
  display: flex;
  flex-direction: column;
  min-height: 0;
  flex: 1;
  background: #0a0e14;
  border: 1px solid var(--border);
  border-radius: var(--radius);
  overflow: hidden;
}

.output {
  flex: 1;
  min-height: 0;
  overflow-y: auto;
  padding: 8px 10px;
  font-family: var(--font-mono);
  font-size: 0.75rem;
  line-height: 1.45;
  -webkit-overflow-scrolling: touch;
}

.welcome {
  color: var(--muted);
  padding: 1rem 0.5rem;
}

.welcome p {
  margin: 0 0 0.5rem;
  font-size: 0.875rem;
  color: var(--text);
}

.welcome ul {
  margin: 0;
  padding-left: 1.1rem;
  font-size: 0.75rem;
  line-height: 1.6;
}

.line {
  display: grid;
  grid-template-columns: auto auto 1fr;
  gap: 8px;
  align-items: baseline;
  padding: 2px 0;
  word-break: break-all;
}

.time {
  color: #4a5568;
  font-size: 0.625rem;
  white-space: nowrap;
}

.tag {
  font-size: 0.5625rem;
  font-weight: 700;
  text-transform: uppercase;
  letter-spacing: 0.04em;
  padding: 1px 5px;
  border-radius: 3px;
  align-self: start;
}

.text {
  white-space: pre-wrap;
}

.line.rx .tag {
  background: color-mix(in srgb, #22d3ee 25%, transparent);
  color: #67e8f9;
}
.line.rx .text {
  color: #a5f3fc;
}

.line.tx .tag {
  background: color-mix(in srgb, #fbbf24 25%, transparent);
  color: #fcd34d;
}
.line.tx .text {
  color: #fde68a;
}

.line.sys .tag {
  background: var(--surface-2);
  color: var(--muted);
}
.line.sys .text {
  color: var(--muted);
}

.line.urc .tag {
  background: color-mix(in srgb, #c084fc 25%, transparent);
  color: #d8b4fe;
}
.line.urc .text {
  color: #e9d5ff;
}

.line.at .tag {
  background: color-mix(in srgb, #34d399 25%, transparent);
  color: #6ee7b7;
}
.line.at .text {
  color: #a7f3d0;
}

.line.err .tag,
.line.disconnect .tag {
  background: color-mix(in srgb, var(--err) 25%, transparent);
  color: #fca5a5;
}
.line.err .text,
.line.disconnect .text {
  color: #fecaca;
}

.toolbar {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 6px 10px;
  padding: 6px 10px;
  border-top: 1px solid var(--border);
  background: var(--surface);
}

.chk {
  flex-direction: row;
  align-items: center;
  gap: 4px;
  font-size: 0.6875rem;
  color: var(--muted);
}

.chk input {
  width: auto;
}

.dock {
  padding: 8px 10px calc(8px + env(safe-area-inset-bottom, 0px));
  border-top: 1px solid var(--border);
  background: var(--surface-2);
  display: flex;
  flex-direction: column;
  gap: 6px;
}

.mode-row {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 8px;
  flex-wrap: wrap;
}

.modes {
  display: flex;
  gap: 4px;
}

.modes button {
  padding: 4px 10px;
  font-size: 0.6875rem;
  background: transparent;
  border-color: var(--border);
  color: var(--muted);
}

.modes button.active {
  color: var(--text);
  border-color: var(--accent);
  background: color-mix(in srgb, var(--accent) 15%, transparent);
}

.ending {
  width: auto;
  min-width: 4.5rem;
  font-size: 0.6875rem;
}

.input-row {
  display: flex;
  align-items: center;
  gap: 6px;
}

.prompt {
  font-family: var(--font-mono);
  color: var(--accent);
  font-weight: 700;
  flex-shrink: 0;
}

.input-row input {
  flex: 1;
  min-width: 0;
  font-family: var(--font-mono);
  font-size: 0.8125rem;
  padding: 8px 10px;
}

.send-btn {
  flex-shrink: 0;
  padding: 8px 14px;
  display: inline-flex;
  align-items: center;
  gap: 6px;
  min-width: 5.5rem;
  justify-content: center;
}

.send-btn.loading {
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

@media (max-width: 640px) {
  .toolbar button:not(:first-of-type):not(:nth-of-type(2)) {
    font-size: 0.6875rem;
    padding: 4px 8px;
  }

  .output {
    font-size: 0.6875rem;
  }
}
</style>
