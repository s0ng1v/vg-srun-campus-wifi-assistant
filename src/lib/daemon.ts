import { invoke, isTauri } from "@tauri-apps/api/core";

export type GuardPhase =
  | "off"
  | "starting"
  | "waiting_network"
  | "discovering_portal"
  | "authenticating"
  | "online"
  | "suspicious"
  | "reconnecting"
  | "needs_action"
  | "crashed";

export type AccountSummary = {
  id: string;
  username: string;
  enabled: boolean;
  verified: boolean;
  verification_failed: boolean;
  active: boolean;
};

export type PortalSnapshot = {
  host: string;
  ac_id: string;
  display_name: string;
};

export type UserSnapshot = {
  username: string;
  ip: string;
  mac: string;
  online_since: string | null;
  session_seconds: number;
  total_bytes: number;
  balance: number | null;
  product_name: string;
};

export type RuntimeMetrics = {
  process_cpu_percent: number;
  process_memory_bytes: number;
  last_probe_latency_ms: number;
  checks_total: number;
  reconnect_total: number;
};

export type DaemonSnapshot = {
  protocol_version: number;
  daemon_version: string;
  guard_enabled: boolean;
  auto_start_enabled: boolean;
  phase: GuardPhase;
  online: boolean;
  active_account_id: string | null;
  accounts: AccountSummary[];
  portal: PortalSnapshot | null;
  user: UserSnapshot | null;
  metrics: RuntimeMetrics;
  last_check_at: string | null;
  next_retry_at: string | null;
  last_error: string | null;
};

export type AccountDraft = {
  id?: string;
  username: string;
  password?: string;
  enabled: boolean;
};

export type DaemonCommand =
  | { type: "ping" }
  | { type: "get_snapshot" }
  | { type: "set_guard"; enabled: boolean }
  | { type: "set_auto_start"; enabled: boolean }
  | { type: "shutdown" }
  | { type: "check_now" }
  | { type: "switch_account"; id: string }
  | { type: "verify_account"; id: string }
  | { type: "save_account"; account: AccountDraft }
  | { type: "remove_account"; id: string }
  | { type: "set_account_enabled"; id: string; enabled: boolean }
  | { type: "reorder_accounts"; ids: string[] }
  | { type: "set_portal"; portal_url: string };

type CommandResponse = {
  protocol_version: number;
  request_id: number;
  ok: boolean;
  message: string | null;
  snapshot: DaemonSnapshot | null;
};

export type DiscoveredPortal = {
  base_url: string;
  ac_id: string;
  domain: string;
};

export async function getDaemonSnapshot(): Promise<DaemonSnapshot> {
  return invoke<DaemonSnapshot>("get_daemon_snapshot");
}

export async function getOfflineSnapshot(): Promise<DaemonSnapshot> {
  return invoke<DaemonSnapshot>("get_offline_snapshot");
}

export async function sendDaemonCommand(command: DaemonCommand): Promise<DaemonSnapshot | null> {
  const response = await invoke<CommandResponse>("daemon_request", { command });
  if (!response.ok) throw new Error(response.message || "校园网守护操作失败");
  return response.snapshot;
}

export async function startDaemon(): Promise<void> {
  await invoke("start_daemon");
}

export async function installSystemIntegration(): Promise<void> {
  await invoke("install_system_integration");
}

export async function discoverPortal(): Promise<DiscoveredPortal> {
  return invoke<DiscoveredPortal>("discover_portal");
}

export async function openDiagnostics(): Promise<void> {
  await invoke("open_diagnostics");
}

export async function openReleasePage(): Promise<void> {
  await invoke("open_release_page");
}

export function isDesktopApp(): boolean {
  return isTauri();
}

