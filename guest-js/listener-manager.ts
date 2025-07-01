import { UnlistenFn } from '@tauri-apps/api/event';

export class ListenerManager {
  private listeners: Map<string, { unlisten: UnlistenFn; type: 'data' | 'disconnect' }> = new Map();
  private listenerIdCounter: number = 0;

  add(type: 'data' | 'disconnect', unlisten: UnlistenFn): UnlistenFn {
    const id = `${type}_${++this.listenerIdCounter}`;
    this.listeners.set(id, { unlisten, type });
    return () => {
      this.delete(id);
      unlisten();
    };
  }

  set(id: string, listener: { unlisten: UnlistenFn; type: 'data' | 'disconnect' }): UnlistenFn {
    this.listeners.set(id, listener);
    return () => {
      this.delete(id);
      listener.unlisten();
    };
  }

  delete(id: string) {
    this.listeners.delete(id);
  }

  entries() {
    return this.listeners.entries();
  }

  filterByType(type: 'data' | 'disconnect') {
    return Array.from(this.listeners.entries()).filter(([_, l]) => l.type === type);
  }

  all() {
    return Array.from(this.listeners.entries());
  }

  clear() {
    this.listeners.clear();
  }

  getInfo() {
    const all = this.all();
    const data = all.filter(([_, l]) => l.type === 'data');
    const disconnect = all.filter(([_, l]) => l.type === 'disconnect');
    return {
      total: all.length,
      data: data.length,
      disconnect: disconnect.length,
      ids: all.map(([id]) => id)
    };
  }

  get(id: string) {
    return this.listeners.get(id);
  }
} 
