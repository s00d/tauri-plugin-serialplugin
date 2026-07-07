<script setup lang="ts">
defineProps<{
  connected: boolean;
  watching: boolean;
  rts: boolean;
  dtr: boolean;
  cts: boolean;
  dsr: boolean;
  ri: boolean;
  cd: boolean;
  bytesToRead: number;
  bytesToWrite: number;
}>();

const emit = defineEmits<{
  toggleRts: [];
  toggleDtr: [];
  refresh: [];
}>();
</script>

<template>
  <div class="signals">
    <div class="out">
      <button type="button" :disabled="!connected" :class="{ on: rts }" @click="emit('toggleRts')">
        RTS {{ rts ? 'ON' : 'off' }}
      </button>
      <button type="button" :disabled="!connected" :class="{ on: dtr }" @click="emit('toggleDtr')">
        DTR {{ dtr ? 'ON' : 'off' }}
      </button>
    </div>
    <div class="in">
      <span :class="{ on: cts }">CTS</span>
      <span :class="{ on: dsr }">DSR</span>
      <span :class="{ on: ri }">RI</span>
      <span :class="{ on: cd }">CD</span>
    </div>
    <div class="bytes">
      <span>in: {{ bytesToRead }}</span>
      <span>out: {{ bytesToWrite }}</span>
      <button type="button" class="ghost" :disabled="!connected || watching" @click="emit('refresh')">↻</button>
    </div>
    <p v-if="watching" class="hint">Live watch — refresh after disconnect or unwatch</p>
  </div>
</template>

<style scoped>
.signals {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 10px;
  font-family: var(--font-mono);
  font-size: 0.6875rem;
}

.out,
.in {
  display: flex;
  gap: 6px;
}

.in span,
.out button {
  padding: 4px 8px;
  border-radius: 4px;
  background: var(--surface-2);
  border: 1px solid var(--border);
  color: var(--muted);
}

.in span.on,
.out button.on {
  color: #86efac;
  border-color: var(--ok);
  background: color-mix(in srgb, var(--ok) 15%, var(--surface-2));
}

.bytes {
  display: flex;
  align-items: center;
  gap: 10px;
  margin-left: auto;
  color: var(--muted);
}

.hint {
  flex: 1 1 100%;
  margin: 0;
  font-size: 0.6875rem;
  color: var(--muted);
}
</style>
