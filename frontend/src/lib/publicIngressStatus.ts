export type PublicIngressMode = "unconfigured" | "manual" | "managed";
export type PublicIngressPhase =
  | "stopped"
  | "starting"
  | "running"
  | "stopping"
  | "error";
export type StableIngressPhase = "unconfigured" | "pending" | "ready" | "failed";

export type PublicIngressStatus = {
  mode: PublicIngressMode;
  public_url: string;
  stable_url: string;
  tunnel: {
    available: boolean;
    running: boolean;
    phase: PublicIngressPhase;
    public_url: string;
    local_url: string;
    stable_phase: StableIngressPhase;
    last_error?: string;
  };
};

const ACTIVE_PHASES = new Set<PublicIngressPhase>([
  "starting",
  "running",
  "stopping",
]);
const MODES = new Set<PublicIngressMode>(["unconfigured", "manual", "managed"]);
const PHASES = new Set<PublicIngressPhase>([
  "stopped",
  "starting",
  "running",
  "stopping",
  "error",
]);
const STABLE_PHASES = new Set<StableIngressPhase>([
  "unconfigured",
  "pending",
  "ready",
  "failed",
]);

function invalid(): never {
  throw new Error("공개 ingress 상태 응답 계약이 올바르지 않습니다.");
}

function exactObject(
  value: unknown,
  required: readonly string[],
  optional: readonly string[] = []
): Record<string, unknown> {
  if (!value || typeof value !== "object" || Array.isArray(value)) invalid();
  const record = value as Record<string, unknown>;
  const actual = Object.keys(record).sort();
  const allowed = [...required, ...optional].sort();
  if (
    required.some((key) => !(key in record)) ||
    actual.some((key) => !allowed.includes(key))
  ) {
    invalid();
  }
  return record;
}

function exactString(value: unknown): string {
  if (typeof value !== "string") invalid();
  return value;
}

function unionValue<T extends string>(value: unknown, allowed: Set<T>): T {
  if (typeof value !== "string" || !allowed.has(value as T)) invalid();
  return value as T;
}

function exactOrigin(value: string, protocol: "http:" | "https:"): URL {
  let url: URL;
  try {
    url = new URL(value);
  } catch {
    invalid();
  }
  if (
    url.protocol !== protocol ||
    url.username ||
    url.password ||
    url.pathname !== "/" ||
    url.search ||
    url.hash ||
    url.origin !== value
  ) {
    invalid();
  }
  return url;
}

function publicOrigin(value: string): string {
  if (!value) return "";
  const url = exactOrigin(value, "https:");
  const host = url.hostname.toLowerCase();
  const comparisonHost = host.replace(/\.+$/, "");
  if (
    comparisonHost === "localhost" ||
    host === "0.0.0.0" ||
    host === "[::]" ||
    host === "[::1]" ||
    /^127(?:\.|$)/.test(host)
  ) {
    invalid();
  }
  return value;
}

function localOrigin(value: string): string {
  if (!value) return "";
  const url = exactOrigin(value, "http:");
  const port = Number(url.port);
  if (
    url.hostname !== "127.0.0.1" ||
    !Number.isInteger(port) ||
    port < 1 ||
    port > 65535
  ) {
    invalid();
  }
  return value;
}

function staticStatusIsExact(status: PublicIngressStatus, manual: boolean): boolean {
  return (
    status.stable_url === "" &&
    status.tunnel.available === false &&
    status.tunnel.running === false &&
    status.tunnel.phase === "stopped" &&
    status.tunnel.public_url === status.public_url &&
    status.tunnel.local_url === "" &&
    status.tunnel.stable_phase === "unconfigured" &&
    status.tunnel.last_error === undefined &&
    (manual ? status.public_url !== "" : status.public_url === "")
  );
}

export function parsePublicIngressStatus(value: unknown): PublicIngressStatus {
  const source = exactObject(value, ["mode", "public_url", "stable_url", "tunnel"]);
  const tunnelSource = exactObject(
    source.tunnel,
    [
      "available",
      "running",
      "phase",
      "public_url",
      "local_url",
      "stable_phase",
    ],
    ["last_error"]
  );
  if (
    typeof tunnelSource.available !== "boolean" ||
    typeof tunnelSource.running !== "boolean"
  ) {
    invalid();
  }
  const lastError =
    tunnelSource.last_error === undefined
      ? undefined
      : exactString(tunnelSource.last_error);
  if (lastError !== undefined && !lastError.trim()) {
    invalid();
  }
  const status: PublicIngressStatus = {
    mode: unionValue(source.mode, MODES),
    public_url: publicOrigin(exactString(source.public_url)),
    stable_url: publicOrigin(exactString(source.stable_url)),
    tunnel: {
      available: tunnelSource.available,
      running: tunnelSource.running,
      phase: unionValue(tunnelSource.phase, PHASES),
      public_url: publicOrigin(exactString(tunnelSource.public_url)),
      local_url: localOrigin(exactString(tunnelSource.local_url)),
      stable_phase: unionValue(tunnelSource.stable_phase, STABLE_PHASES),
      ...(lastError === undefined ? {} : { last_error: lastError }),
    },
  };
  if (status.mode === "unconfigured") {
    if (!staticStatusIsExact(status, false)) invalid();
    return status;
  }
  if (status.mode === "manual") {
    if (!staticStatusIsExact(status, true)) invalid();
    return status;
  }
  const active = ACTIVE_PHASES.has(status.tunnel.phase);
  if (
    status.tunnel.running !== active ||
    (active && !status.tunnel.available) ||
    status.public_url !== status.tunnel.public_url ||
    (status.public_url && status.tunnel.phase !== "running") ||
    (status.tunnel.phase === "error" && !status.tunnel.last_error) ||
    (status.tunnel.stable_phase === "failed" && !status.tunnel.last_error)
  ) {
    invalid();
  }
  if (!status.tunnel.local_url) invalid();
  if (status.tunnel.stable_phase !== "ready" && status.stable_url) invalid();
  if (
    status.stable_url &&
    (!status.public_url || status.tunnel.phase !== "running")
  ) {
    invalid();
  }
  if (
    status.tunnel.stable_phase === "ready" &&
    !status.stable_url &&
    (status.public_url ||
      (status.tunnel.phase !== "stopped" && status.tunnel.phase !== "error"))
  ) {
    invalid();
  }
  return status;
}
