// DAEMON wave ("browser.diagnostics"): curated ubb_* definition for the
// multi-browser read-only scan. The wrapper tool `browser.diagnostics` lives in
// the extension (CORE-2, lib/tools-lifecycle.js → requestDaemon('diag.browsers'));
// the actual scanning is the daemon's job (daemon/diag.mjs).
import { s } from './schema.mjs';

export const DIAGNOSTICS_TOOLS = [
  {
    name: 'ubb_diagnostics',
    description: 'READ-ONLY multi-browser diagnostics — no session, no browser actions, no token needed. Reports, per Chromium family (edge/chrome/brave/opera/vivaldi): installed (exe), running (process), whether the unpacked UBB extension is installed and enabled in which profiles, and whether the native-messaging manifest com.ubb.bridge resolves to a live executable; plus the live daemon status (port, extension connected, slots). Use when the extension is not connected (EXTENSION_NOT_CONNECTED) to locate the breakage instead of guessing.',
    inputSchema: { type: 'object', properties: {}, additionalProperties: true },
    ubb: 'browser.diagnostics',
  },
];
