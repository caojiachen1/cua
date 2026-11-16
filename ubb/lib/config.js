// UBB shared configuration and protocol constants.

export const PROTOCOL = {
  version: '1.0',
  minCompatible: '1.0',
  client: 'ubb-extension',
};

export const STORAGE_KEYS = {
  token: 'ubb_token',
  port: 'ubb_port',
  enabled: 'ubb_enabled',
  shieldMode: 'ubb_shield_mode', // 'block' | 'border' | 'off'
  browserName: 'ubb_browser_name', // which daemon slot this profile registers under
  workspace: 'ubb_workspace', // true for browser.launch / auto-spawned agent workspaces
};

/** A browser slot name: [a-z0-9_-]{1,32}. One profile of one browser binary
 *  registers under exactly one name; `browser.launch` pre-names the fresh
 *  profiles it creates, and the popup can rename any profile. */
export const BROWSER_NAME_RE = /^[a-z0-9_-]{1,32}$/;

export const DEFAULTS = {
  port: 8765,
  enabled: true,
  shieldMode: 'block',
  browserName: 'default',
};

export const IDLE_DETACH_MS = 30_000;
export const HEARTBEAT_MS = 20_000;
export const HEARTBEAT_TIMEOUT_MS = 15_000;
export const DEFAULT_TOOL_TIMEOUT_MS = 40_000;
export const SCREENSHOT_TOOL_TIMEOUT_MS = 60_000;
export const TRACE_TOOL_TIMEOUT_MS = 120_000;
// Per-CDP-command deadline: bounds a hung renderer command so it cannot wedge
// the per-tab queue. Above the tool timeouts (40/60s) that agents see first;
// below TRACE (120s) — tracing data arrives via events, never a held command.
export const CDP_CMD_TIMEOUT_MS = 45_000;
export const BUILD = '2026.11.14-codex-absorb';
// Default byte budget for one snapshot page. The old char-only cap let a
// CJK-heavy page blow the model's context with far fewer "chars" than it
// looks; a byte cap is what actually bounds the payload.
export const SNAPSHOT_MAX_BYTES = 16_384;

export const GROUP_COLOR = 'purple';
export const GROUP_TITLES = {
  running: '⏳ UBB',
  done: '✅ UBB',
  error: '❌ UBB',
  paused: '⏸ UBB',
  kept: '📌 UBB', // session.stop 保留了 marked (deliverable/handoff) tab
};

export function err(code, message, data) {
  return { code, message: message || code, ...(data ? { data } : {}) };
}

export function randomHex(bytes) {
  const a = new Uint8Array(bytes);
  crypto.getRandomValues(a);
  return Array.from(a, (b) => b.toString(16).padStart(2, '0')).join('');
}
