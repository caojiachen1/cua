// Offscreen video recorder: screencast JPEG frames (base64, from the service
// worker's CDP Page.screencastFrame hook) → ImageBitmap → VideoFrame →
// WebCodecs VideoEncoder (VP8) → lib/webm-muxer.js → WebM bytes.
//
// Message protocol (chrome.runtime.sendMessage, type 'CUA_VIDEO'):
//   {op:'start', rec_id, width?, height?}   → {ok, supported, codec?} | {ok:false, error}
//   {op:'frame', b64, ts, w, h}             → {ok, queued} | {ok:false, error, fatal?}
//   {op:'status'}                           → {ok, recording, frames, dropped, width, height, error?}
//   {op:'stop'}                             → {ok, frames, bytes, width, height, error?}
//   {op:'drain'}                            → {ok, chunk_b64|null, done, bytes_left}
//   {op:'abort'}                            → {ok:true}
//
// Honest failure shapes: a frame that cannot decode is counted in `dropped`,
// never silently skipped; a size change mid-recording is FATAL (VP8 cannot
// reconfigure mid-stream here) and surfaces through status/stop as error.

import { WebmMuxer } from '../lib/webm-muxer.js';

const KEYFRAME_INTERVAL_FRAMES = 60; // ≈2s at 30fps, ≥1 GOP per 2s at any rate
const MAX_DROPPED_RATIO = 0.5; // more than half the frames failing → stop honestly
const RECORDER_CODE = 'shots-v1';
const MAX_QUEUE = 120; // bounded backpressure: drop NEWEST frames past this

const state = {
  recording: false,
  recId: null,
  muxer: null,
  encoder: null,
  queue: [],
  pumping: false,
  frames: 0,
  dropped: 0,
  width: 0,
  height: 0,
  framesSinceKey: 0,
  fatal: null,
  result: null, // Uint8Array once stopped
  drainPos: 0,
  lastTsUs: 0,
};

function b64ToBytes(b64) {
  const bin = atob(b64);
  const out = new Uint8Array(bin.length);
  for (let i = 0; i < bin.length; i++) out[i] = bin.charCodeAt(i);
  return out;
}

function bytesToB64(bytes) {
  let s = '';
  const CH = 0x8000;
  for (let i = 0; i < bytes.length; i += CH) {
    s += String.fromCharCode.apply(null, bytes.subarray(i, i + CH));
  }
  return btoa(s);
}

async function pickCodec(width, height) {
  const candidates = [
    { codec: 'vp8', width, height, bitrate: 2_500_000, framerate: 30, latencyMode: 'quality' },
    { codec: 'vp09.00.10.08', width, height, bitrate: 2_500_000, framerate: 30, latencyMode: 'quality' },
  ];
  for (const cfg of candidates) {
    try {
      const support = await VideoEncoder.isConfigSupported(cfg);
      if (support?.supported) return cfg;
    } catch {
      /* try the next */
    }
  }
  return null;
}

async function startEncoding(width, height) {
  const cfg = await pickCodec(width, height);
  if (!cfg) throw new Error('WebCodecs VideoEncoder supports neither vp8 nor vp09 in this browser build');
  state.muxer = new WebmMuxer({ width, height });
  state.encoder = new VideoEncoder({
    output: (chunk, meta) => {
      const bytes = new Uint8Array(chunk.byteLength);
      chunk.copyTo(bytes);
      const keyframe = chunk.type === 'key' || meta?.decoderConfig?.description != null || (meta && meta.duration !== undefined && chunk.type === 'key');
      state.muxer.addBlock(bytes, Math.round(chunk.timestamp / 1000), chunk.type === 'key');
      void keyframe;
    },
    error: (e) => {
      state.fatal = String(e?.message || e);
    },
  });
  state.encoder.configure(cfg);
  state.width = width;
  state.height = height;
  return cfg.codec;
}

async function pump() {
  if (state.pumping) return;
  state.pumping = true;
  try {
    while (state.queue.length) {
      if (state.fatal) {
        state.queue.length = 0;
        break;
      }
      const item = state.queue.shift();
      try {
        const bytes = b64ToBytes(item.b64);
        const blob = new Blob([bytes], { type: 'image/jpeg' });
        const bitmap = await createImageBitmap(blob);
        state.lastSize = [bitmap.width, bitmap.height];
        // screencast emits a 0x0 "gone" frame when the stream ends — not a
        // size change, just the tail. Drop it and keep recording.
        if (!(bitmap.width > 0) || !(bitmap.height > 0)) {
          bitmap.close();
          state.dropped += 1;
          continue;
        }
        if (state.width > 0 && state.height > 0 && (bitmap.width !== state.width || bitmap.height !== state.height)) {
          bitmap.close();
          if (state.tolerant) {
            // screenshot sampling can hand back a frame whose edge rounding
            // differs by a pixel; drop it instead of killing the file
            state.dropped += 1;
            continue;
          }
          state.fatal = `frame size changed mid-recording: ${state.width}x${state.height} → ${bitmap.width}x${bitmap.height} (VP8 cannot reconfigure; the recording stopped at the last good frame)`;
          break;
        }
        const frame = new VideoFrame(bitmap, { timestamp: Math.round(item.ts * 1e6) });
        bitmap.close();
        const keyFrame = state.framesSinceKey >= KEYFRAME_INTERVAL_FRAMES;
        state.encoder.encode(frame, { keyFrame });
        frame.close();
        state.framesSinceKey = keyFrame ? 0 : state.framesSinceKey + 1;
        state.frames += 1;
      } catch (e) {
        state.dropped += 1;
        if (!state.tolerant && state.dropped > 0 && state.dropped / Math.max(1, state.frames + state.dropped) > MAX_DROPPED_RATIO) {
          state.fatal = `more than ${Math.round(MAX_DROPPED_RATIO * 100)}% of frames failed to decode (${state.dropped} of ${state.frames + state.dropped}) — stopping honestly instead of shipping a broken file`;
          break;
        }
      }
      // yield so the message pump never starves
      await new Promise((r) => setTimeout(r, 0));
    }
    if (state.fatal && state.encoder && state.encoder.state === 'configured') {
      try { await state.encoder.flush(); } catch { /* fatal already recorded */ }
      try { state.encoder.close(); } catch {}
      state.encoder = null;
      finalizeResult();
    }
  } finally {
    state.pumping = false;
  }
}

function finalizeResult() {
  if (state.result || !state.muxer) return;
  try {
    // timestamps are epoch-based; duration is the SPAN, not the last absolute value
    state.result = state.muxer.finish();
  } catch (e) {
    state.fatal = String(e?.message || e);
    state.result = null;
  }
}

async function stopRecording() {
  try {
    if (state.encoder && state.encoder.state === 'configured') {
      await state.encoder.flush();
      state.encoder.close();
    }
  } catch (e) {
    if (!state.fatal) state.fatal = String(e?.message || e);
  }
  state.encoder = null;
  // drain any queued frames even after a fatal, so frames counts stay honest
  await pump();
  finalizeResult();
  state.recording = false;
  return {
    frames: state.frames,
    dropped: state.dropped,
    bytes: state.result ? state.result.length : 0,
    width: state.width,
    height: state.height,
    error: state.fatal || undefined,
  };
}

chrome.runtime.onMessage.addListener((msg, _sender, sendResponse) => {
  if (msg?.type !== 'CUA_VIDEO') return;
  (async () => {
    switch (msg.op) {
      case 'start': {
        if (state.recording) return { ok: false, error: 'a recording is already active in this document' };
        Object.assign(state, {
          recording: true,
          recId: msg.rec_id || 'v?',
          muxer: null,
          encoder: null,
          queue: [],
          pumping: false,
          frames: 0,
          dropped: 0,
          width: msg.width | 0,
          height: msg.height | 0,
          framesSinceKey: 0,
          fatal: null,
          result: null,
          drainPos: 0,
          lastTsUs: 0,
          lastSize: null,
          tolerant: msg.tolerant === true,
        });
        if (!('VideoEncoder' in globalThis)) {
          state.recording = false;
          return { ok: false, error: 'WebCodecs VideoEncoder is not available in this browser build — video recording is unsupported here', unsupported: true };
        }
        // width/height may be 0 (defined by the first frame)
        if (state.width > 0 && state.height > 0) {
          try {
            const codec = await startEncoding(state.width, state.height);
            return { ok: true, codec };
          } catch (e) {
            state.recording = false;
            return { ok: false, error: String(e?.message || e), unsupported: true };
          }
        }
        return { ok: true, codec: null, note: 'dimensions resolve from the first frame' };
      }
      case 'frame': {
        if (!state.recording) return { ok: false, error: 'no active recording', fatal: true };
        if (state.fatal) return { ok: false, error: state.fatal, fatal: true };
        if (state.dropped / Math.max(1, state.frames + state.dropped) > MAX_DROPPED_RATIO) {
          state.fatal = 'too many undecodable frames — recording stopped';
          return { ok: false, error: state.fatal, fatal: true };
        }
        if (state.queue.length >= MAX_QUEUE) {
          // drop the NEWEST frame: the encoder is behind, and a growing queue
          // would silently blow the offscreen document's memory
          return { ok: true, queued: false, dropped: true };
        }
        state.queue.push({ b64: msg.b64, ts: msg.ts || 0, w: msg.w | 0, h: msg.h | 0 });
        if (msg.ts) state.lastTsUs = Math.max(state.lastTsUs, Math.round(msg.ts * 1e6));
        if (!state.muxer && (msg.w | 0) > 0 && (msg.h | 0) > 0) {
          try {
            await startEncoding(msg.w | 0, msg.h | 0);
          } catch (e) {
            state.fatal = String(e?.message || e);
            return { ok: false, error: state.fatal, fatal: true };
          }
        }
        pump();
        return { ok: true, queued: true };
      }
      case 'probe_image': {
        try {
          const bytes = b64ToBytes(msg.b64);
          const bmp = await createImageBitmap(new Blob([bytes], { type: 'image/jpeg' }));
          const out = { ok: true, width: bmp.width, height: bmp.height };
          bmp.close();
          return out;
        } catch (e) {
          return { ok: false, error: String(e?.message || e) };
        }
      }
      case 'status': {
        return {
          ok: true,
          page: location.pathname,
          recording: state.recording,
          rec_id: state.recId,
          code: RECORDER_CODE,
          last_size: state.lastSize || null,
          frames: state.frames,
          dropped: state.dropped,
          width: state.width,
          height: state.height,
          queued: state.queue.length,
          error: state.fatal || undefined,
        };
      }
      case 'stop': {
        const r = await stopRecording();
        return { ok: true, ...r };
      }
      case 'drain': {
        if (!state.result) return { ok: true, chunk_b64: null, done: true, bytes_left: 0 };
        const CH = 1_048_576;
        const end = Math.min(state.result.length, state.drainPos + CH);
        const slice = state.result.subarray(state.drainPos, end);
        state.drainPos = end;
        return {
          ok: true,
          chunk_b64: bytesToB64(slice),
          done: state.drainPos >= state.result.length,
          bytes_left: state.result.length - state.drainPos,
        };
      }
      case 'abort': {
        try { if (state.encoder) state.encoder.close(); } catch {}
        state.encoder = null;
        state.recording = false;
        state.result = null;
        state.muxer = null;
        state.queue.length = 0;
        return { ok: true };
      }
      default:
        return { ok: false, error: `unknown op ${msg.op}` };
    }
  })().then(sendResponse).catch((e) => sendResponse({ ok: false, error: String(e?.message || e) }));
  return true; // async sendResponse
});
