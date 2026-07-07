<script setup lang="ts">
import { DataBits, FlowControl, Parity, StopBits } from 'tauri-plugin-serialplugin-api';

defineProps<{
  connected: boolean;
  baudRate: number;
  dataBits: DataBits;
  flowControl: FlowControl;
  parity: Parity;
  stopBits: StopBits;
  timeout: number;
  flushMs: number;
  autoReconnect: boolean;
}>();

const emit = defineEmits<{
  'update:baudRate': [v: number];
  'update:dataBits': [v: DataBits];
  'update:flowControl': [v: FlowControl];
  'update:parity': [v: Parity];
  'update:stopBits': [v: StopBits];
  'update:timeout': [v: number];
  'update:flushMs': [v: number];
  'update:autoReconnect': [v: boolean];
  apply: [];
}>();
</script>

<template>
  <div class="settings">
    <div class="grid">
      <label>
        Baud
        <input
          type="number"
          :value="baudRate"
          @input="emit('update:baudRate', +($event.target as HTMLInputElement).value)"
        />
      </label>
      <label>
        Data bits
        <select
          :value="dataBits"
          @change="emit('update:dataBits', ($event.target as HTMLSelectElement).value as DataBits)"
        >
          <option v-for="v in [DataBits.Five, DataBits.Six, DataBits.Seven, DataBits.Eight]" :key="v" :value="v">
            {{ v }}
          </option>
        </select>
      </label>
      <label>
        Parity
        <select
          :value="parity"
          @change="emit('update:parity', ($event.target as HTMLSelectElement).value as Parity)"
        >
          <option v-for="v in [Parity.None, Parity.Odd, Parity.Even]" :key="v" :value="v">{{ v }}</option>
        </select>
      </label>
      <label>
        Stop bits
        <select
          :value="stopBits"
          @change="emit('update:stopBits', ($event.target as HTMLSelectElement).value as StopBits)"
        >
          <option v-for="v in [StopBits.One, StopBits.Two]" :key="v" :value="v">{{ v }}</option>
        </select>
      </label>
      <label>
        Flow
        <select
          :value="flowControl"
          @change="emit('update:flowControl', ($event.target as HTMLSelectElement).value as FlowControl)"
        >
          <option v-for="v in [FlowControl.None, FlowControl.Software, FlowControl.Hardware]" :key="v" :value="v">
            {{ v }}
          </option>
        </select>
      </label>
      <label>
        Timeout ms
        <input
          type="number"
          :value="timeout"
          @input="emit('update:timeout', +($event.target as HTMLInputElement).value)"
        />
      </label>
      <label>
        Flush ms
        <input
          type="number"
          min="10"
          max="2000"
          :value="flushMs"
          @input="emit('update:flushMs', +($event.target as HTMLInputElement).value)"
        />
      </label>
      <label class="check">
        <input
          type="checkbox"
          :checked="autoReconnect"
          :disabled="connected"
          @change="emit('update:autoReconnect', ($event.target as HTMLInputElement).checked)"
        />
        Auto-reconnect
      </label>
    </div>
    <button type="button" class="ghost" :disabled="!connected" @click="emit('apply')">Apply to open port</button>
  </div>
</template>

<style scoped>
.settings {
  display: flex;
  flex-direction: column;
  gap: 8px;
}

.grid {
  display: grid;
  grid-template-columns: repeat(4, minmax(0, 1fr));
  gap: 8px;
}

.check {
  flex-direction: row;
  align-items: center;
  align-self: end;
  gap: 8px;
  color: var(--text);
}

.check input {
  width: auto;
}

@media (max-width: 900px) {
  .grid {
    grid-template-columns: repeat(2, 1fr);
  }
}
</style>
