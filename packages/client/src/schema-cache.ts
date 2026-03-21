import type { SchemaHandle, QuantaDecoder } from "@quanta/delta-decoder";

/**
 * Caches parsed SchemaHandle instances keyed by a string identifier
 * (typically "namespace:type"). Falls back to localStorage for persistence
 * of the raw QSCH bytes across page reloads.
 */
export class SchemaCache {
  private handles = new Map<string, SchemaHandle>();
  private storagePrefix: string;

  constructor(storagePrefix = "quanta:schema:") {
    this.storagePrefix = storagePrefix;
  }

  /** Get or create a SchemaHandle from raw QSCH bytes. */
  getOrCreate(key: string, bytes: Uint8Array, decoder: QuantaDecoder): SchemaHandle {
    const existing = this.handles.get(key);
    if (existing) return existing;

    const handle = decoder.createSchema(bytes);
    this.handles.set(key, handle);
    this.persistBytes(key, bytes);
    return handle;
  }

  /** Try to restore a schema from localStorage. Returns null if not found. */
  restore(key: string, decoder: QuantaDecoder): SchemaHandle | null {
    const existing = this.handles.get(key);
    if (existing) return existing;

    const bytes = this.loadBytes(key);
    if (!bytes) return null;

    try {
      const handle = decoder.createSchema(bytes);
      this.handles.set(key, handle);
      return handle;
    } catch {
      this.removeBytes(key);
      return null;
    }
  }

  /** Remove a cached schema. */
  remove(key: string): void {
    const handle = this.handles.get(key);
    if (handle) {
      handle.free();
      this.handles.delete(key);
    }
    this.removeBytes(key);
  }

  /** Clear all cached schemas and their localStorage entries. */
  clear(): void {
    for (const [key, handle] of this.handles) {
      handle.free();
      this.removeBytes(key);
    }
    this.handles.clear();
  }

  private persistBytes(key: string, bytes: Uint8Array): void {
    try {
      const CHUNK = 8192;
      let binary = "";
      for (let i = 0; i < bytes.length; i += CHUNK) {
        binary += String.fromCharCode(...bytes.subarray(i, i + CHUNK));
      }
      localStorage.setItem(this.storagePrefix + key, btoa(binary));
    } catch {
      // localStorage may be unavailable (SSR, quota exceeded)
    }
  }

  private loadBytes(key: string): Uint8Array | null {
    try {
      const stored = localStorage.getItem(this.storagePrefix + key);
      if (!stored) return null;
      const binary = atob(stored);
      const bytes = new Uint8Array(binary.length);
      for (let i = 0; i < binary.length; i++) {
        bytes[i] = binary.charCodeAt(i);
      }
      return bytes;
    } catch {
      return null;
    }
  }

  private removeBytes(key: string): void {
    try {
      localStorage.removeItem(this.storagePrefix + key);
    } catch {
      // ignore
    }
  }
}
