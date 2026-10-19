import { vi } from "vitest";

// A minimal in-memory IndexedDB for the browser storage the member resume key
// uses. jsdom has no IndexedDB and the packaged app verifies the real browser
// storage; this shim only lets the storage contract be exercised here.
type Row = Record<string, unknown>;

class FakeRequest<T> {
  onsuccess: (() => void) | null = null;
  onerror: (() => void) | null = null;
  onupgradeneeded: (() => void) | null = null;
  result: T = undefined as T;
  error: unknown = null;
}

class FakeStore {
  readonly rows = new Map<string, Row>();

  constructor(private readonly keyPath: string) {}

  get(key: string): FakeRequest<Row | undefined> {
    const request = new FakeRequest<Row | undefined>();
    queueMicrotask(() => {
      request.result = this.rows.get(key);
      request.onsuccess?.();
    });
    return request;
  }

  getAll(): FakeRequest<Row[]> {
    const request = new FakeRequest<Row[]>();
    queueMicrotask(() => {
      request.result = [...this.rows.values()];
      request.onsuccess?.();
    });
    return request;
  }

  put(row: Row): void {
    this.rows.set(String(row[this.keyPath]), { ...row });
  }

  delete(key: string): void {
    this.rows.delete(key);
  }
}

class FakeTransaction {
  oncomplete: (() => void) | null = null;
  onerror: (() => void) | null = null;
  onabort: (() => void) | null = null;
  error: unknown = null;

  constructor(private readonly store: FakeStore) {
    queueMicrotask(() => queueMicrotask(() => this.oncomplete?.()));
  }

  objectStore(): FakeStore {
    return this.store;
  }
}

class FakeDatabase {
  readonly stores = new Map<string, FakeStore>();
  readonly objectStoreNames = { contains: (name: string) => this.stores.has(name) };

  transaction(): FakeTransaction {
    const store = [...this.stores.values()][0];
    if (!store) throw new Error("the fake database has no store");
    return new FakeTransaction(store);
  }

  createObjectStore(name: string, options: { keyPath: string }): FakeStore {
    const store = new FakeStore(options.keyPath);
    this.stores.set(name, store);
    return store;
  }

  close(): void {}
}

/** Installs the fake IndexedDB and returns the stored rows of every store. */
export function installFakeIndexedDb(): () => Map<string, Row>[] {
  const databases = new Map<string, FakeDatabase>();
  vi.stubGlobal("indexedDB", {
    open(name: string) {
      const request = new FakeRequest<FakeDatabase>();
      let database = databases.get(name);
      if (!database) {
        database = new FakeDatabase();
        databases.set(name, database);
      }
      queueMicrotask(() => {
        request.result = database;
        request.onupgradeneeded?.();
        request.onsuccess?.();
      });
      return request;
    },
  });
  return () =>
    [...databases.values()].flatMap((database) =>
      [...database.stores.values()].map((store) => store.rows),
    );
}
