// Minimal WebM (Matroska subset) muxer for ONE VP8 video track.
// Zero dependencies, on purpose: the same file the offscreen recorder imports
// is what daemon/test-video.mjs unit-tests against real EBML grammar — the
// bytes are the contract, not a screenshot of a player.

// ------------------------------------------------------------------ EBML atoms

export const ID = {
  EBML: [0x1a, 0x45, 0xdf, 0xa3],
  Segment: [0x18, 0x53, 0x80, 0x67],
  Info: [0x15, 0x49, 0xa9, 0x66],
  TimecodeScale: [0x2a, 0xd7, 0xb1],
  Duration: [0x44, 0x89],
  MuxingApp: [0x4d, 0x80],
  WritingApp: [0x57, 0x41],
  Tracks: [0x16, 0x54, 0xae, 0x6b],
  TrackEntry: [0xae],
  TrackNumber: [0xd7],
  TrackUID: [0x73, 0xc5],
  TrackType: [0x83],
  CodecID: [0x86],
  Video: [0xe0],
  PixelWidth: [0xb0],
  PixelHeight: [0xba],
  Cluster: [0x1f, 0x43, 0xb6, 0x75],
  Timecode: [0xe7],
  SimpleBlock: [0xa3],
};

/** Concatenate byte arrays. */
export function concat(parts) {
  let n = 0;
  for (const p of parts) n += p.length;
  const out = new Uint8Array(n);
  let o = 0;
  for (const p of parts) {
    out.set(p, o);
    o += p.length;
  }
  return out;
}

/**
 * EBML variable-length size. A k-byte vint marks its length with the first
 * `k` leading zeros absent — i.e. first byte = marker | value's top bits.
 * All-ones (e.g. 0xFF) is the reserved "unknown" form and is never produced
 * for a known size; callers wanting unknown size use UNKNOWN_SIZE.
 */
export function ebmlSize(value) {
  // the upper bound is 2^56-2, but `2**56 - 2` rounds to 2**56 in float64 (the
  // spacing at that magnitude is already 4) — compare exactly via BigInt or the
  // guard silently admits the very values it exists to refuse
  if (!Number.isInteger(value) || value < 0 || BigInt(value) > (1n << 56n) - 2n) {
    throw new Error(`ebmlSize: out of range ${value}`);
  }
  let len = 1;
  while (len < 8 && value >= 2 ** (7 * len) - 1) len += 1;
  const out = new Uint8Array(len);
  out[0] = (1 << (8 - len)) | (Math.floor(value / 2 ** (8 * (len - 1))) & (0xff >> len));
  for (let i = 1; i < len; i++) {
    out[i] = Math.floor(value / 2 ** (8 * (len - 1 - i))) & 0xff;
  }
  return out;
}

export const UNKNOWN_SIZE = new Uint8Array([0x01, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff]);

/** Unsigned integer payload, minimal big-endian (0 encodes as a single 0x00). */
export function uintPayload(value) {
  if (!Number.isInteger(value) || value < 0) throw new Error(`uintPayload: ${value}`);
  if (value === 0) return new Uint8Array([0]);
  const bytes = [];
  let v = value;
  while (v > 0) {
    bytes.unshift(v & 0xff);
    v = Math.floor(v / 256);
  }
  return new Uint8Array(bytes);
}

/** float64 payload (Info.Duration). */
export function floatPayload(value) {
  const buf = new ArrayBuffer(8);
  new DataView(buf).setFloat64(0, value);
  return new Uint8Array(buf);
}

export function asciiPayload(s) {
  const out = new Uint8Array(s.length);
  for (let i = 0; i < s.length; i++) out[i] = s.charCodeAt(i) & 0xff;
  return out;
}

/** One master element: id + size + children bytes. */
export function element(idBytes, payload) {
  return concat([new Uint8Array(idBytes), ebmlSize(payload.length), payload]);
}

/** One SimpleBlock payload: track vint + int16 BE relative timecode + flags + data. */
export function simpleBlockPayload(frameBytes, relTimecodeMs, keyframe) {
  const head = new Uint8Array(4);
  head[0] = 0x81; // track number 1, vint
  const rel = Math.max(-32768, Math.min(32767, Math.round(relTimecodeMs))) & 0xffff;
  head[1] = (rel >> 8) & 0xff;
  head[2] = rel & 0xff;
  head[3] = keyframe ? 0x80 : 0x00; // bit 7 = keyframe
  return concat([head, frameBytes]);
}

// ------------------------------------------------------------------ muxer

const CLUSTER_MAX_MS = 20_000; // split long silences too, not just data bursts
const CLUSTER_MAX_BYTES = 8 * 1024 * 1024;
const KEYFRAME_INTERVAL_MS = 2000; // encoder side uses ~this many frames

export class WebmMuxer {
  /**
   * @param {object} opts
   * @param {number} opts.width   pixel width of the single video track
   * @param {number} opts.height  pixel height
   */
  constructor({ width, height }) {
    if (!Number.isInteger(width) || width <= 0 || !Number.isInteger(height) || height <= 0) {
      throw new Error(`WebmMuxer: width/height must be positive integers, got ${width}x${height}`);
    }
    this.width = width;
    this.height = height;
    this.frames = 0;
    this.bytes = 0; // encoded payload bytes fed in (not container overhead)
    this.finished = false;
    this._chunks = [];
    this._openCluster = null; // { startMs, parts, bytes }
    this._firstTsMs = null;
    this._lastTsMs = null;
    this._durationOffset = null; // { chunkIdx, offset } patch site for Info.Duration

    // --- header: EBML + Segment(unknown size) + Info + Tracks
    const ebml = element(ID.EBML, concat([
      element([0x42, 0x86], uintPayload(1)), // EBMLVersion
      element([0x42, 0xf7], uintPayload(1)), // EBMLReadVersion
      element([0x42, 0xf2], uintPayload(4)), // EBMLMaxIDLength
      element([0x42, 0xf3], uintPayload(8)), // EBMLMaxSizeLength
      element([0x42, 0x82], asciiPayload('webm')), // DocType
      element([0x42, 0x87], uintPayload(2)), // DocTypeVersion
      element([0x42, 0x85], uintPayload(2)), // DocTypeReadVersion
    ]));

    // Duration FIRST inside Info so its patch site is at a known offset
    const infoPayload = concat([
      element(ID.Duration, floatPayload(0)),
      element(ID.TimecodeScale, uintPayload(1_000_000)), // 1 ms
      element(ID.MuxingApp, asciiPayload('cua-extension')),
      element(ID.WritingApp, asciiPayload('cua-extension')),
    ]);
    const info = element(ID.Info, infoPayload);

    const tracks = element(ID.Tracks, element(ID.TrackEntry, concat([
      element(ID.TrackNumber, uintPayload(1)),
      element(ID.TrackUID, uintPayload(1)),
      element(ID.TrackType, uintPayload(1)), // 1 = video; Chrome's demuxer refuses a track without it
      element(ID.CodecID, asciiPayload('V_VP8')),
      element(ID.Video, concat([
        element(ID.PixelWidth, uintPayload(this.width)),
        element(ID.PixelHeight, uintPayload(this.height)),
      ])),
    ])));

    const segHeader = concat([
      new Uint8Array(ID.Segment),
      UNKNOWN_SIZE,
      info,
      tracks,
    ]);
    this._chunks.push(ebml, segHeader);
    // Duration float lives at: len(ebml) + 4(id) + 8(unknown size) + 4(info id)
    //   + sizeVint(infoPayload.length) + 2(duration id) + 1(duration size vint)
    this._durationOffset = {
      chunkIdx: 1,
      offset: 4 + 8 + 4 + ebmlSize(infoPayload.length).length + 2 + 1,
    };
  }

  /**
   * Append one encoded VP8 frame.
   * @param {Uint8Array} data  encoded frame bytes
   * @param {number} timestampMs  frame presentation timestamp in ms (from the
   *                              screencast metadata; monotonic, may have gaps)
   * @param {boolean} keyframe
   */
  addBlock(data, timestampMs, keyframe) {
    if (this.finished) throw new Error('WebmMuxer: already finished');
    if (this._firstTsMs === null) this._firstTsMs = timestampMs;
    if (this._lastTsMs !== null && timestampMs < this._lastTsMs) {
      throw new Error(`WebmMuxer: timestamps must be non-decreasing (${timestampMs} < ${this._lastTsMs})`);
    }
    this._lastTsMs = timestampMs;
    // Screencast timestamps are epoch-based. Cluster timecodes are relative to
    // the SEGMENT start, so normalize to 0 here; leaving epoch values makes a
    // player report a ~1.8e9 s duration (and breaks seeking).
    const relTs = timestampMs - this._firstTsMs;
    // close the open cluster BEFORE the block joins it: the SimpleBlock
    // relative timecode is int16 (±32s), so a cluster that already spans
    // ~20s (or 8MB) must not swallow a block that lands even later
    if (
      this._openCluster &&
      (relTs - this._openCluster.startMs >= CLUSTER_MAX_MS || this._openCluster.bytes >= CLUSTER_MAX_BYTES)
    ) {
      this._closeCluster();
    }
    if (!this._openCluster) this._openCluster = { startMs: relTs, parts: [], bytes: 0 };
    const rel = relTs - this._openCluster.startMs;
    const block = element(ID.SimpleBlock, simpleBlockPayload(data, rel, keyframe));
    this._openCluster.parts.push(block);
    this._openCluster.bytes += block.length;
    this.frames += 1;
    this.bytes += data.length;
  }

  _closeCluster() {
    const c = this._openCluster;
    if (!c || !c.parts.length) {
      this._openCluster = null;
      return;
    }
    const cluster = element(ID.Cluster, concat([
      element(ID.Timecode, uintPayload(Math.max(0, Math.round(c.startMs)))),
      ...c.parts,
    ]));
    this._chunks.push(cluster);
    this._openCluster = null;
  }

  /**
   * Finalize: close the last cluster, patch Info.Duration, return the file.
   * @param {number} totalMs  overall duration hint (last timestamp is used when absent)
   * @returns {Uint8Array}
   */
  finish(totalMs) {
    if (this.finished) throw new Error('WebmMuxer: already finished');
    this._closeCluster();
    const duration = Math.max(
      0,
      (totalMs ?? (this._lastTsMs != null && this._firstTsMs != null ? this._lastTsMs - this._firstTsMs : 0)),
    );
    const file = concat(this._chunks);
    const abs = this._chunks.slice(0, this._durationOffset.chunkIdx).reduce((n, c) => n + c.length, 0) + this._durationOffset.offset;
    new DataView(file.buffer, file.byteOffset, file.byteLength).setFloat64(abs, duration);
    this.finished = true;
    this.durationMs = duration;
    return file;
  }
}
