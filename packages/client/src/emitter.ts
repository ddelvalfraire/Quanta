export class TypedEmitter<Events extends { [K in keyof Events]: (...args: any[]) => void }> {
  private listeners = new Map<keyof Events, Set<(...args: unknown[]) => void>>();

  on<E extends keyof Events>(event: E, callback: Events[E]): void {
    let set = this.listeners.get(event);
    if (!set) {
      set = new Set();
      this.listeners.set(event, set);
    }
    set.add(callback as (...args: unknown[]) => void);
  }

  off<E extends keyof Events>(event: E, callback: Events[E]): void {
    this.listeners.get(event)?.delete(callback as (...args: unknown[]) => void);
  }

  protected emit<E extends keyof Events>(event: E, ...args: Parameters<Events[E]>): void {
    const set = this.listeners.get(event);
    if (!set) return;
    for (const cb of set) {
      try {
        cb(...args);
      } catch {
        // Don't let listener errors break the emitter owner
      }
    }
  }

  protected clearListeners(): void {
    this.listeners.clear();
  }
}
