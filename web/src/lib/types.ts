// 管理 API 的响应形状（与 xpxe-control 的 JSON 一致：时间为 Unix 秒；节点状态为小写）。
export type NodeState = "idle" | "armed" | "installing" | "installed" | "configuring" | "ready" | "failed";

export interface Channel {
  type: "lan" | "redfish";
  bmc?: string;
}

export interface NodeSpec {
  hostname: string;
  fqdn: string | null;
  mac: string;
  os_family: string;
  firmware: "uefi" | "bios";
  channel: Channel;
  network: { type: "dhcp" } | { type: "static"; address: string; gateway: string; dns: string[] };
}

export interface NodeRow {
  id: number;
  hostname: string;
  mac: string;
  state: NodeState;
  state_since: number;
  deadline_at: number | null;
  active_session_id: string | null;
  channel: Channel;
  firmware: "uefi" | "bios";
  network: NodeSpec["network"];
  profile_id: number;
  init_plan_id: number;
  alert: string | null;
  last_event_id: string | null;
  spec: NodeSpec;
  spec_object_id: number;
}

export interface EventDetail {
  id: number;
  node_id: number;
  hostname: string;
  session_id: string | null;
  event_id: string;
  at: number;
  source: string;
  kind: string;
  from_state: NodeState | null;
  to_state: NodeState | null;
  payload: unknown;
}

export interface ConfigObject {
  id: number;
  kind: string;
  name: string;
  head_version: number;
}

export interface VersionRef {
  object_id: number;
  version: number;
  content_sha256: string;
}

export interface ConfigHead<T = Record<string, unknown>> {
  document: T;
  version: VersionRef;
}

export interface VersionInfo {
  version: number;
  parent_version: number | null;
  author_id: number | null;
  comment: string;
  created_at: number;
  content_sha256: string;
}

export interface FieldIssue {
  path: string;
  code: string;
  message: string;
}

export interface Artifact {
  name: string;
  text: string;
}

export interface DiffLineDto {
  kind: "same" | "add" | "del";
  text: string;
}

export interface Change {
  path: string;
  before: string;
  after: string;
}

export interface RenderedDiff {
  name: string;
  lines: DiffLineDto[];
}

export interface PreviewResult {
  ok: boolean;
  fields: FieldIssue[];
  artifacts?: Artifact[];
  modules?: Artifact[];
  plan?: unknown;
  package_count?: number;
  warnings?: FieldIssue[];
  changes?: Change[];
  rendered_diff?: RenderedDiff[];
}

export interface AssetRecord {
  id: number;
  kind: string;
  label: string;
  sha256: string;
  size: number;
  source: string;
  source_url: string | null;
  kernel_abi: string | null;
  verify: Record<string, unknown> | null;
  stage: string;
  created_at: number;
  created_by: string | null;
}

export interface AssetsView {
  assets: AssetRecord[];
  /** 当前激活通道（控制面原样序列化 `Channels { channels }`）。 */
  channels: { channels: Record<string, { asset_id: number; sha256: string } | undefined> };
}

export interface Heartbeat {
  at: number;
  version: string;
  rev: number;
  mode: string;
  health: string;
  detail: string | null;
  listening: number[];
  counters: Record<string, number>;
  probe: ProbeReport | null;
}

export interface Responder {
  server: string;
  class: "address_only" | "pxe_capable" | "proxy";
  siaddr: string;
  file: string | null;
  options: number[];
}

export interface ProbeReport {
  at: number;
  responders: Responder[];
  conflicts: string[];
  external: "correct" | "missing_next_server" | "file_mismatch" | "no_dhcp" | null;
}

export interface BootnetStatus {
  healthy: boolean;
  rev: number;
  heartbeat: { heartbeat: Heartbeat; received_at: number } | null;
}

export interface Observation {
  at: number;
  mac: string;
  arch: string;
  user_class: string;
  kind: string;
  detail: string;
}

export interface Snippets {
  server: string;
  snippets: { kind: string; text: string }[];
  external: unknown;
}

export interface HistoryRow {
  kind: string;
  object_id: number;
  name: string;
  head_version: number;
  version: number;
  parent_version: number | null;
  author: string | null;
  created_at: number;
  comment: string;
  content_sha256: string;
}

export interface AuditEntry {
  id: number;
  at: string | number;
  actor: string;
  ip: string;
  action: string;
  target: string;
  outcome: string;
  detail: unknown;
}

export interface AuditPage {
  total: number;
  entries: AuditEntry[];
  actors: string[];
  actions: string[];
}

export interface Me {
  user: { id: number; username: string; role: string; must_change_password: boolean };
  role: string;
}

export interface UserSummary {
  id: number;
  username: string;
  role: string;
  disabled: boolean;
  must_change_password: boolean;
  created_at: number;
}

export interface SessionInfo {
  key: string;
  created_at: number;
  last_seen_at: number;
  expires_at: number;
  ip: string;
  user_agent: string;
  current: boolean;
}

export interface TokenInfo {
  id: number;
  name: string;
  prefix?: string;
  scopes: string[];
  created_at: number;
  expires_at: number;
  last_used_at?: number | null;
}

export interface Overview {
  now: number;
  states: { by_state: Partial<Record<NodeState, number>>; idle: number };
  outcomes: { day: string; ready: number; failed: number }[];
  events: EventDetail[];
}

export interface InitModuleInfo {
  order: number;
  id: string;
  title: string;
  default_timeout_s: number;
  defaults: ModuleSlot | null;
}

export interface ModuleSlot {
  enabled?: boolean;
  continue_on_error?: boolean;
  timeout_s?: number | null;
  settings?: Record<string, unknown>;
}

export interface InitModules {
  modules: InitModuleInfo[];
  package_groups: { id: string; count: number; packages: string[] }[];
  sysctl_presets: Record<string, unknown>;
}

export interface Presets {
  profile: Record<string, Record<string, unknown>>;
  init_plan: Record<string, Record<string, unknown>>;
}

export interface SystemInfo {
  version: string;
  boot_base_url: string;
  https: boolean;
  offline: boolean;
  bootnet_allowed_ifaces?: string[];
  [key: string]: unknown;
}

export interface SessionDetail {
  id: string;
  channel: string;
  expires_at: number;
  closed_at: number | null;
  outcome: string | null;
  preseed_served_at: number | null;
  plan_served_at: number | null;
  pinned: Record<string, unknown>;
  token_tail: string | null;
  log: string;
  events: EventDetail[];
}
