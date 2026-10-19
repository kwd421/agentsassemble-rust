// The member resume key: one non-extractable P-256 key per person, server
// and registration epoch, kept in the browser's own secure storage.
//
// The private key never leaves this module and is never written to
// localStorage, to the member anchor or to the central device key. It is
// deleted when the account changes or the session ends, and the in-memory
// handle is dropped in every tab when the session is cleared.
import { encodeBase64Url } from "../base64Url";
import { CENTRAL_SESSION_CLEARED_EVENT } from "./centralSessionEvents";

const DB_NAME = "agentsassemble-member-resume-v1";
const STORE_NAME = "resume-keys";
const DB_VERSION = 1;

export type MemberResumeKey = {
  privateKey: CryptoKey;
  /** The raw uncompressed public key (65 bytes), base64url encoded. */
  publicKey: string;
};

type StoredResumeKey = {
  key: string;
  person_id: string;
  server_id: string;
  registration_epoch: string;
  privateKey: CryptoKey;
  publicKey: string;
};

const memory = new Map<string, MemberResumeKey>();
let listenersBound = false;

function storageKey(personId: string, serverId: string, registrationEpoch: string): string {
  return `${personId}\u0000${serverId}\u0000${registrationEpoch}`;
}

function openDatabase(): Promise<IDBDatabase> {
  return new Promise((resolve, reject) => {
    const request = indexedDB.open(DB_NAME, DB_VERSION);
    request.onupgradeneeded = () => {
      const database = request.result;
      if (!database.objectStoreNames.contains(STORE_NAME)) {
        database.createObjectStore(STORE_NAME, { keyPath: "key" });
      }
    };
    request.onsuccess = () => resolve(request.result);
    request.onerror = () =>
      reject(request.error || new Error("이 브라우저에서 재연결 자격 증명을 저장하지 못했어요."));
  });
}

function request<T>(operation: IDBRequest<T>): Promise<T> {
  return new Promise((resolve, reject) => {
    operation.onsuccess = () => resolve(operation.result);
    operation.onerror = () =>
      reject(operation.error || new Error("이 브라우저에서 재연결 자격 증명을 저장하지 못했어요."));
  });
}

function transactionDone(transaction: IDBTransaction): Promise<void> {
  return new Promise((resolve, reject) => {
    transaction.oncomplete = () => resolve();
    transaction.onerror = () =>
      reject(transaction.error || new Error("이 브라우저에서 재연결 자격 증명을 저장하지 못했어요."));
    transaction.onabort = () =>
      reject(transaction.error || new Error("이 브라우저에서 재연결 자격 증명을 저장하지 못했어요."));
  });
}

/** Drops every in-memory handle. The stored keys stay until they are deleted. */
export function forgetMemberResumeKeyHandles(): void {
  memory.clear();
}

function bindSessionListeners(): void {
  if (listenersBound) return;
  listenersBound = true;
  if (typeof window === "undefined") return;
  // The clearing tab drops its own handle, and every other tab hears the
  // storage event of the cleared session.
  window.addEventListener(CENTRAL_SESSION_CLEARED_EVENT, forgetMemberResumeKeyHandles);
  window.addEventListener("storage", forgetMemberResumeKeyHandles);
}

/**
 * The stored resume key of one device on one host, if this account has one.
 * The key is created on the first call that needs it.
 */
export async function loadOrCreateMemberResumeKey(
  personId: string,
  serverId: string,
  registrationEpoch: string,
): Promise<MemberResumeKey> {
  bindSessionListeners();
  if (!personId || !serverId || !registrationEpoch) {
    throw new Error("재연결 자격 증명을 확인할 수 없어요.");
  }
  const key = storageKey(personId, serverId, registrationEpoch);
  const cached = memory.get(key);
  if (cached) return cached;
  const database = await openDatabase();
  try {
    const store = database.transaction(STORE_NAME, "readwrite").objectStore(STORE_NAME);
    const existing = (await request(store.get(key))) as StoredResumeKey | undefined;
    if (existing?.privateKey) {
      const value = { privateKey: existing.privateKey, publicKey: existing.publicKey };
      memory.set(key, value);
      return value;
    }
    // Extractable is false: the private half can sign and can never be
    // exported, copied to another device or read from storage.
    const pair = await crypto.subtle.generateKey(
      { name: "ECDSA", namedCurve: "P-256" },
      false,
      ["sign"],
    );
    const publicKey = encodeBase64Url(
      await crypto.subtle.exportKey("raw", pair.publicKey),
    );
    const record: StoredResumeKey = {
      key,
      person_id: personId,
      server_id: serverId,
      registration_epoch: registrationEpoch,
      privateKey: pair.privateKey,
      publicKey,
    };
    const transaction = database.transaction(STORE_NAME, "readwrite");
    transaction.objectStore(STORE_NAME).put(record);
    await transactionDone(transaction);
    const value = { privateKey: pair.privateKey, publicKey };
    memory.set(key, value);
    return value;
  } finally {
    database.close();
  }
}

/** The stored resume key of one device, without creating one. */
export async function loadMemberResumeKey(
  personId: string,
  serverId: string,
  registrationEpoch: string,
): Promise<MemberResumeKey | null> {
  bindSessionListeners();
  if (!personId || !serverId || !registrationEpoch) return null;
  const key = storageKey(personId, serverId, registrationEpoch);
  const cached = memory.get(key);
  if (cached) return cached;
  const database = await openDatabase();
  try {
    const stored = (await request(
      database.transaction(STORE_NAME, "readonly").objectStore(STORE_NAME).get(key),
    )) as StoredResumeKey | undefined;
    if (!stored?.privateKey) return null;
    const value = { privateKey: stored.privateKey, publicKey: stored.publicKey };
    memory.set(key, value);
    return value;
  } catch {
    return null;
  } finally {
    database.close();
  }
}

/**
 * Deletes every resume key of one person. A key never outlives the account
 * that owns it, so the next person on this browser starts without one.
 */
export async function clearMemberResumeKeys(personId: string): Promise<void> {
  forgetMemberResumeKeyHandles();
  if (!personId) return;
  let database: IDBDatabase;
  try {
    database = await openDatabase();
  } catch {
    return;
  }
  try {
    const read = database.transaction(STORE_NAME, "readonly");
    const records = (await request(
      read.objectStore(STORE_NAME).getAll(),
    )) as StoredResumeKey[];
    // The deletes run in their own transaction: a readwrite transaction does
    // not stay open across an awaited read.
    const write = database.transaction(STORE_NAME, "readwrite");
    const store = write.objectStore(STORE_NAME);
    for (const record of records) {
      if (record.person_id === personId) store.delete(record.key);
    }
    await transactionDone(write);
  } catch {
    // The session clearing stays authoritative even when the browser storage
    // cannot be reached.
  } finally {
    database.close();
  }
}
