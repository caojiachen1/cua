// video.start / video.status / video.stop — record a tab's screencast into a
// WebM file. The tab's single screencast slot is taken through the REAL
// screencast.start tool (owner 'video'), so SCREENCAST_BUSY/lease semantics
// stay in exactly one place. Frames flow through hookCdpEvents into the
// offscreen recorder document, which encodes VP8 via WebCodecs and muxes WebM.

import { err } from './config.js';
import { cdp } from './cdp.js';
import { targetTab, evalJs } from './actions.js';
import { hookCdpEvents } from './tools.js';
import { audit } from './audit.js';
import { redact } from './redact.js';

const DEFAULT_MAX_SECONDS = 120;
const MAX_SECONDS = 280; // under the 300s screencast lease ceiling on purpose — but "shots" needs no lease, so it may run longer
const MAX_SECONDS_SHOTS = 1800; // 30 min for screenshot-sampled recordings (batch runs, soak tests)
const MAX_RESULT_BYTES = 192 * 1024 * 1024; // drain ceiling; bigger recordings are refused honestly
const RECORDER_CODE = 'shots-v1'; // must match offscreen/recorder.js

const recordings = new Map(); // tabId → { recId, started_at, started_perf, max_ms, unhook, timer, stopping }
// the last recording's end state lives here even after the map entry is gone —
// a recording that auto-stopped (fatal frame/resize) must stay explainable
let lastVideoEnd = null; // { rec_id, tab_id, reason, error?, frames, at }

async function ensureRecorderDoc() {
  // ONE offscreen document for the whole extension. An extension reload does
  // NOT always kill the old offscreen page — a surviving doc from an older
  // build answers the probe with stale code, so the probe must name its page;
  // anything other than the merged index.html is closed and rebuilt.
  const probe = await chrome.runtime.sendMessage({ type: 'CUA_VIDEO', op: 'status' }).catch(() => null);
  if (probe && probe.ok && probe.page === '/offscreen/index.html' && probe.code === RECORDER_CODE) return;
  if (await chrome.offscreen.hasDocument()) {
    await chrome.offscreen.closeDocument().catch(() => {});
    await new Promise((r) => setTimeout(r, 250));
  }
  await chrome.offscreen.createDocument({
    url: 'offscreen/index.html',
    reasons: ['CLIPBOARD', 'USER_MEDIA'],
    justification: 'clipboard access and tab-screencast video encoding for cua agent tools',
  });
  await new Promise((r) => setTimeout(r, 300)); // let the recorder listener attach
}

async function askRecorder(msg) {
  const r = await chrome.runtime.sendMessage({ type: 'CUA_VIDEO', ...msg });
  if (!r) throw err('RECORDER_GONE', 'the offscreen recorder document did not answer');
  return r;
}

async function releaseSlot(dispatcher, tabId) {
  const stop = dispatcher.tools.get('screencast.stop');
  if (stop) {
    await stop.fn({ tab_id: tabId, owner: 'video' }).catch(() => {});
  } else {
    await cdp.send(tabId, 'Page.stopScreencast').catch(() => {});
  }
}

export function register(dispatcher) {
  const R = (name, meta, fn) => dispatcher.register(name, meta, fn);

  R('video.start', { category: 'capture', timeoutMs: 30_000, description: 'Record the tab into a WebM video (VP8 via WebCodecs in an offscreen document). source:"screencast" (default) uses the real screencast slot; source:"shots" samples Page.captureScreenshot at interval_ms (default 125ms) instead — it needs no visible compositor surface, so it records an occluded/minimized window honestly at a lower frame rate. max_seconds clamps 1-280; finish with video.stop and save with --out. Takes the tab\'s single screencast slot as owner "video" through the real screencast.start semantics — a live cast is refused with SCREENCAST_BUSY unless force:true. max_seconds clamps 1-280 (default 120; the auto-stop at max_seconds is honest: the file says exactly what was captured). Frames the recorder cannot decode are counted (dropped), never skipped silently; a mid-recording viewport resize is FATAL and stops the file at the last good frame. Finish with video.stop — the tool result carries data_base64 (use the CLI --out to save the file).' }, async (p = {}) => {
    const tabId = targetTab(p);
    if (recordings.has(tabId)) {
      const r = recordings.get(tabId);
      throw err('VIDEO_RECORDING_ACTIVE', `tab ${tabId} is already recording (${r.recId}, since ${r.started_at}) — call video.stop first`);
    }
    await ensureRecorderDoc();
    const maxSeconds = Math.min(Math.max(Number(p.max_seconds) || DEFAULT_MAX_SECONDS, 1), p.source === "shots" ? MAX_SECONDS_SHOTS : MAX_SECONDS);

    // --- screenshot-sampled recording (no compositor surface required) --------
    if (p.source === 'shots') {
      const vp = await evalJs(tabId, '[innerWidth, innerHeight]').catch(() => null);
      const vw = Array.isArray(vp) ? Number(vp[0]) : 0;
      const vh = Array.isArray(vp) ? Number(vp[1]) : 0;
      if (!(vw > 100 && vh > 100)) {
        throw err('VIEWPORT_ZERO', `viewport is ${vw}x${vh} — screenshots would be empty. Restore/resize the window first (window.restore + resize_window, or dev.focus {restore_window:true}).`);
      }
      // No clip/scale: a scaled clip on an occluded surface can come back
      // black. Set the viewport with `emulate` before recording and capture it
      // 1:1 — the encoder is then configured with the PROBED size.
      const q = Math.min(Math.max(p.quality ?? 55, 10), 90);
      const interval = Math.min(Math.max(Number(p.interval_ms) || 125, 60), 1000);
      const capture = () => cdp.send(tabId, 'Page.captureScreenshot', { format: 'jpeg', quality: q });
      let firstShot = null;
      let dims = null;
      for (let i = 0; i < 12 && !dims; i++) {
        const s = await capture().catch(() => null);
        if (s?.data) {
          const pr = await askRecorder({ op: 'probe_image', b64: s.data }).catch(() => null);
          if (pr?.ok && pr.width > 0 && pr.height > 0) { dims = { w: pr.width, h: pr.height }; firstShot = s; break; }
        }
        await new Promise((r) => setTimeout(r, 120));
      }
      if (!dims) throw err('RECORDER_FAILED', 'the first screenshot decoded to 0x0 — the tab has no paintable surface yet; bring it to the foreground or wait for a paint, then retry');
      const outW = dims.w;
      const outH = dims.h;
      const recId = `v${Date.now().toString(36)}-${tabId}`;
      const first = await askRecorder({ op: 'start', rec_id: recId, width: outW, height: outH, tolerant: true });
      if (!first.ok) throw err(first.unsupported ? 'VIDEO_UNSUPPORTED' : 'RECORDER_FAILED', first.error || 'recorder refused to start');
      if (firstShot) await askRecorder({ op: 'frame', b64: firstShot.data, ts: Date.now() / 1000, w: outW, h: outH });
      const st = {
        recId, source: 'shots', started_at: new Date().toISOString(), started_perf: Date.now(),
        max_ms: maxSeconds * 1000, unhook: null, timer: null, stopping: false,
        shotTimer: null, shotBusy: false, shotW: outW, shotH: outH,
      };
      const tick = async () => {
        if (st.stopping || st.shotBusy) return;
        st.shotBusy = true;
        try {
          const shot = await capture();
          if (shot?.data) {
            await askRecorder({ op: 'frame', b64: shot.data, ts: Date.now() / 1000, w: outW, h: outH });
          }
        } catch { /* a dropped sample is counted by the recorder only if it arrived */ }
        finally { st.shotBusy = false; }
      };
      st.unhook = () => { if (st.shotTimer) clearInterval(st.shotTimer); st.shotTimer = null; };
      st.shotTimer = setInterval(tick, interval);
      tick();
      st.timer = setTimeout(() => { stopRecording(dispatcher, tabId, { auto: true }).catch(() => {}); }, st.max_ms);
      recordings.set(tabId, st);
      audit.log({ action: 'video.start', tabId, rec_id: recId, source: 'shots', max_seconds: maxSeconds, fps: Math.round(1000 / interval) });
      return redact({
        recording: true, source: 'shots', rec_id: recId, tab_id: tabId,
        max_seconds: maxSeconds, interval_ms: interval, size: [outW, outH],
        note: 'screenshot-sampled recording — finish with video.stop (data_base64 via --out)',
      });
    }
    // take the slot THROUGH the real tool: lease/BUSY semantics stay canonical
    const start = dispatcher.tools.get('screencast.start');
    if (!start) throw err('UNKNOWN_TOOL', 'screencast.start is not registered — cannot take the capture slot');
    const slot = await start.fn({
      tab_id: tabId,
      owner: 'video',
      quality: Math.min(Math.max(p.quality ?? 60, 10), 90),
      max_width: p.max_width ?? 1280,
      max_height: p.max_height ?? 720,
      lease_ms: 300_000,
      force: p.force === true,
    });
    if (slot && slot.error) throw err('SCREENCAST_BUSY', slot.error.message || 'screencast slot unavailable');
    try {
      const recId = `v${Date.now().toString(36)}-${tabId}`;
      const first = await askRecorder({ op: 'start', rec_id: recId, width: p.width | 0, height: p.height | 0 });
      if (!first.ok) {
        await releaseSlot(dispatcher, tabId);
        throw err(first.unsupported ? 'VIDEO_UNSUPPORTED' : 'RECORDER_FAILED', first.error || 'recorder refused to start');
      }
      const st = {
        recId,
        started_at: new Date().toISOString(),
        started_perf: Date.now(),
        max_ms: maxSeconds * 1000,
        unhook: null,
        timer: null,
        stopping: false,
      };
      st.unhook = hookCdpEvents((source, method, params) => {
        if (source.tabId !== tabId || method !== 'Page.screencastFrame') return;
        const rec = recordings.get(tabId);
        if (!rec || rec.stopping) return;
        // ack first: the browser throttles unacked frames to a trickle
        cdp.send(tabId, 'Page.screencastFrameAck', { sessionId: params.sessionId }).catch(() => {});
        // zero-sized frames are the stream's tail/heartbeat, not content —
        // forwarding one made the recorder declare a fatal size change
        if (!(params.metadata?.deviceWidth > 0) || !(params.metadata?.deviceHeight > 0)) return;
        askRecorder({ op: 'frame', b64: params.data, ts: params.metadata?.timestamp || 0, w: params.metadata?.deviceWidth | 0, h: params.metadata?.deviceHeight | 0 })
          .then((r) => {
            if (r && r.fatal) {
              // the recorder gave up (resize/decode collapse): stop and keep the partial file
              stopRecording(dispatcher, tabId, { auto: true }).catch(() => {});
            }
          })
          .catch(() => {});
      });
      st.timer = setTimeout(() => {
        stopRecording(dispatcher, tabId, { auto: true }).catch(() => {});
      }, st.max_ms);
      recordings.set(tabId, st);
      audit.log({ action: 'video.start', tabId, rec_id: recId, max_seconds: maxSeconds });
      return redact({
        recording: true,
        rec_id: recId,
        tab_id: tabId,
        max_seconds: maxSeconds,
        slot_owner: 'video',
        ...(slot.reclaimed_from ? { reclaimed_from: slot.reclaimed_from } : {}),
        note: `recording — call video.stop within ${maxSeconds}s (auto-stops at the cap); the file lands as data_base64 on video.stop (save it with the CLI --out)`,
      });
    } catch (e) {
      await releaseSlot(dispatcher, tabId);
      throw e;
    }
  });

  R('video.reset', {
    category: 'diagnostics',
    description: 'Close the shared offscreen document so the next video.start recreates it from the CURRENT recorder.js/webm-muxer.js on disk. Needed after editing recorder code: an offscreen page survives an extension reload and keeps serving the old module.',
  }, async () => {
    const had = await chrome.offscreen.hasDocument().catch(() => false);
    if (had) await chrome.offscreen.closeDocument().catch(() => {});
    return { closed: had, note: 'next video.start rebuilds /offscreen/index.html from disk' };
  });

  R('video.status', { category: 'capture', description: 'Recording state per tab: {recordings:[{tab_id, rec_id, started_at, elapsed_s, max_seconds}]} plus the recorder document\'s own counters (frames, dropped, width, height, error) when one is active.' }, async (p = {}) => {
    const rows = [];
    for (const [tabId, st] of recordings) {
      const stats = await askRecorder({ op: 'status' }).catch(() => null);
      rows.push({
        tab_id: tabId,
        rec_id: st.recId,
        source: st.source || 'screencast',
        started_at: st.started_at,
        elapsed_s: Math.round((Date.now() - st.started_perf) / 1000),
        max_seconds: Math.round(st.max_ms / 1000),
        ...(stats && stats.ok ? { frames: stats.frames, dropped: stats.dropped, width: stats.width, height: stats.height, error: stats.error } : {}),
      });
    }
    return { recordings: rows, ...(lastVideoEnd && !rows.length ? { last_end: lastVideoEnd } : {}) };
  });

  async function stopRecording(dispatcherRef, tabId, { auto = false } = {}) {
    const st = recordings.get(tabId);
    if (!st) return { recording: false };
    if (st.stopping) throw err('VIDEO_STOPPING', 'a stop is already in flight for this tab');
    st.stopping = true;
    if (st.timer) clearTimeout(st.timer);
    if (st.unhook) st.unhook();
    if (st.source === 'shots') {
      // let a screenshot already in flight land before finalizing
      for (let i = 0; i < 20 && st.shotBusy; i++) await new Promise((r) => setTimeout(r, 25));
    }
    const stats = await askRecorder({ op: 'stop' }).catch((e) => ({ ok: false, error: String(e?.message || e) }));
    if (st.source !== 'shots') await releaseSlot(dispatcherRef, tabId);
    recordings.delete(tabId);
    lastVideoEnd = {
      rec_id: st.recId,
      tab_id: tabId,
      reason: auto ? 'auto-stop (max_seconds or recorder fatal)' : 'video.stop',
      error: stats?.error,
      frames: stats?.frames,
      at: new Date().toISOString(),
    };
    audit.log({ action: auto ? 'video.autostop' : 'video.stop', tab_id: tabId, rec_id: st.recId, frames: stats.frames, error: stats?.error });
    return { stats, st, auto };
  }

  R('video.stop', { category: 'capture', timeoutMs: 180_000, description: 'Stop the tab\'s video recording and return the WebM file: {stopped, frames, dropped, bytes, width, height, duration_s, data_base64} (save with the CLI --out; over MCP the file lands under ~/.cua/videos). dropped counts frames that failed to decode; a non-empty error field means the recording ended early (resize is fatal) and the file covers only what was encoded. Recordings over the drain ceiling are refused with VIDEO_TOO_BIG instead of pretending.' }, async (p = {}) => {
    const tabId = targetTab(p);
    const st = recordings.get(tabId);
    if (!st) {
      return {
        recording: false,
        ...(lastVideoEnd && lastVideoEnd.tab_id === tabId ? { last_end: lastVideoEnd } : {}),
        note: 'no active recording on this tab (video.start first, or it already auto-stopped at max_seconds — last_end explains why when it did)',
      };
    }
    const { stats } = await stopRecording(dispatcher, tabId);
    const durationS = (Date.now() - st.started_perf) / 1000;
    if (!stats || stats.ok === false) {
      throw err('RECORDER_FAILED', stats?.error || 'the recorder document failed to finalize the file');
    }
    if (stats.bytes > MAX_RESULT_BYTES) {
      throw err('VIDEO_TOO_BIG', `the recording is ${Math.round(stats.bytes / 1048576)}MB, over the ${Math.round(MAX_RESULT_BYTES / 1048576)}MB drain ceiling — lower max_seconds/quality; nothing was returned`);
    }
    // drain the finalized file in bounded chunks
    const parts = [];
    let total = 0;
    for (;;) {
      const r = await askRecorder({ op: 'drain' });
      if (!r || !r.ok) throw err('RECORDER_FAILED', 'drain failed');
      if (r.chunk_b64 == null) break;
      parts.push(r.chunk_b64);
      total += r.chunk_b64.length;
      if (r.done) break;
    }
    // The recorder chunks the RAW bytes at 1MiB and base64s each slice
    // independently; 1MiB % 3 !== 0 means every non-final chunk ends with
    // padding that is INVALID mid-stream (Rust CLI: "base64 decode failed:
    // Invalid symbol 61"). Strip the inter-chunk padding before joining.
    let data_base64;
    if (parts.length === 1) {
      data_base64 = parts[0];
    } else {
      const clean = parts.map((b64, i) => {
        if (i === parts.length - 1) return b64; // final chunk keeps its padding
        return b64.replace(/=+$/, ''); // mid-stream padding out
      });
      data_base64 = clean.join('');
    }
    return redact({
      stopped: true,
      rec_id: st.recId,
      frames: stats.frames,
      dropped: stats.dropped,
      bytes: stats.bytes,
      width: stats.width,
      height: stats.height,
      duration_s: Math.round(durationS * 10) / 10,
      chunks: parts.length,
      b64_len: data_base64.length,
      format: 'webm',
      filename_suggested: `cua-${st.recId}.webm`,
      ...(stats.error ? { error: stats.error, note: 'the recording ended early — the file covers only what was encoded before the fatal condition' } : {}),
      data_base64,
    });
  });
}
