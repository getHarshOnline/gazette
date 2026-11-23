import { requireThat } from './contract.mjs';

export class CDP {
  constructor(socket) {
    this.socket = socket; this.next = 0; this.pending = new Map(); this.listeners = new Set();
    socket.addEventListener('message', event => {
      try {
        requireThat(typeof event.data === 'string' && Buffer.byteLength(event.data) <= 96 << 20, 'browser-message-limit');
        const value = JSON.parse(event.data);
        if (value.id) {
          const pending = this.pending.get(value.id);
          if (!pending) return;
          this.pending.delete(value.id);
          if (value.error) pending.reject(new Error('browser-command-failed'));
          else pending.resolve(value.result);
        } else for (const listener of this.listeners) listener(value);
      } catch (error) { this.abort(error); }
    });
    socket.addEventListener('close', () => this.abort(new Error('browser-disconnected')));
    socket.addEventListener('error', () => this.abort(new Error('browser-connection-failed')));
  }
  static async connect(address) {
    const url = new URL(address);
    requireThat(url.protocol === 'ws:' && ['127.0.0.1', 'localhost', '[::1]'].includes(url.hostname), 'invalid-browser-endpoint');
    const socket = new WebSocket(address);
    await new Promise((resolve, reject) => {
      socket.addEventListener('open', resolve, { once: true });
      socket.addEventListener('error', () => reject(new Error('browser-connection-failed')), { once: true });
    });
    return new CDP(socket);
  }
  send(method, params = {}, sessionId) {
    requireThat(this.socket.readyState === WebSocket.OPEN, 'browser-disconnected');
    const id = ++this.next;
    return new Promise((resolve, reject) => {
      this.pending.set(id, { resolve, reject });
      this.socket.send(JSON.stringify({ id, method, params, ...(sessionId ? { sessionId } : {}) }));
    });
  }
  abort(error) {
    for (const pending of this.pending.values()) pending.reject(error);
    this.pending.clear();
  }
  close() { this.abort(new Error('browser-closed')); this.socket.close(); }
}
