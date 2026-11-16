// video.* offline suite: the WebM muxer is a pure module, so the container
// grammar is asserted byte-for-byte here — the live encoder is exercised by
// e2e/run.mjs (a real WebCodecs pass), but a broken muxer must never need a
// browser to be found. Run: node daemon/test-video.mjs

import assert from 'node:assert/strict';
import {
  WebmMuxer, ebmlSize, uintPayload, floatPayload, element, simpleBlockPayload, UNKNOWN_SIZE, ID,
} from '../lib/webm-muxer.js';

let passed = 0;
let failed = 0;
const failures = [];
function ok(name, cond, extra) {
  if (cond) {
    passed += 1;
    console.log(`  ✓ ${name}`);
  } else {
    failed += 1;
    failures.push(name);
    console.log(`  ✗ ${name}${extra ? ` — ${extra}` : ''}`);
  }
}

function hex(u8) {
  return Buffer.from(u8).toString('hex');
}

console.log('\n[1] EBML primitives');
ok('size 0 → 0x80 (1-byte vint marker)', hex(ebmlSize(0)) === '80');
ok('size 126 → 0xfe (max of 1-byte range)', hex(ebmlSize(126)) === 'fe');
ok('size 127 spills to 2 bytes (0xFF is reserved)', hex(ebmlSize(127)) === '407f');
ok('size 16382 → 7ffe (max of 2-byte range)', hex(ebmlSize(16382)) === '7ffe');
ok('size 16383 → 3 bytes', hex(ebmlSize(16383)) === '203fff');
ok('UNKNOWN_SIZE is the 8-byte all-ones vint', hex(UNKNOWN_SIZE) === '01ffffffffffffff');
ok('uintPayload(0) is a single 0x00', hex(uintPayload(0)) === '00');
ok('uintPayload(1_000_000) is 3-byte big-endian', hex(uintPayload(1_000_000)) === '0f4240');
ok('floatPayload writes IEEE754 float64', Buffer.from(floatPayload(21000)).readDoubleBE(0) === 21000);
assert.throws(() => ebmlSize(-1), /out of range/);
assert.throws(() => ebmlSize(2 ** 56), /out of range/);
ok('ebmlSize refuses negatives and >56-bit values', true);
ok('SimpleBlock payload: track vint + int16 rel + flags + data', (() => {
  const p = simpleBlockPayload(new Uint8Array([0xaa, 0xbb]), 1234, true);
  return hex(p.slice(0, 4)) === '8104d280' && hex(p.slice(4)) === 'aabb';
})());
ok('SimpleBlock negative relative timecode wraps int16', (() => {
  const p = simpleBlockPayload(new Uint8Array([]), -2, false);
  return hex(p.slice(1, 3)) === 'fffe' && p[3] === 0x00;
})());
ok('element() = id + size + payload', (() => {
  const e = element(ID.Timecode, uintPayload(0));
  return hex(e) === 'e78100';
})());

console.log('\n[2] container structure');
const m = new WebmMuxer({ width: 64, height: 48 });
m.addBlock(new Uint8Array([1, 2, 3]), 0, true);
m.addBlock(new Uint8Array([4, 5]), 100, false);
const file = Buffer.from(m.finish(100));
ok('starts with the EBML header id', hex(file.subarray(0, 4)) === '1a45dfa3');
ok('DocType is webm', file.includes(Buffer.from('webm')));
ok('Segment uses the unknown-size vint', file.includes(Buffer.from('01ffffffffffffff', 'hex')));
ok('track codec is V_VP8', file.includes(Buffer.from('V_VP8')));
ok('PixelWidth/PixelHeight come from the constructor', file.includes(Buffer.from([0xb0, 0x81, 0x40])) && file.includes(Buffer.from([0xba, 0x81, 0x30])));
ok('Duration is patched to the finish() value', (() => {
  const at = file.indexOf(Buffer.from([0x44, 0x89, 0x88]));
  return at > 0 && file.readDoubleBE(at + 3) === 100;
})());
ok('TimecodeScale is 1ms (1000000ns)', file.includes(Buffer.from([0x2a, 0xd7, 0xb1, 0x83, 0x0f, 0x42, 0x40])));

console.log('\n[3] cluster splitting');
const m2 = new WebmMuxer({ width: 2, height: 2 });
m2.addBlock(new Uint8Array([1]), 0, true);
m2.addBlock(new Uint8Array([2]), 20_000, true); // ≥ CLUSTER_MAX_MS → new cluster BEFORE this block
m2.addBlock(new Uint8Array([3]), 40_000, true);
const f2 = Buffer.from(m2.finish(40_000));
let clusters = 0;
for (let i = 0; i < f2.length - 3; i++) {
  if (f2[i] === 0x1f && f2[i + 1] === 0x43 && f2[i + 2] === 0xb6 && f2[i + 3] === 0x75) clusters += 1;
}
ok('a >20s gap forces a new cluster (int16 relative timecode stays safe)', clusters === 3, `got ${clusters}`);
ok('cluster timecodes are the first block of each cluster (0, 20000, 40000)', (() => {
  const tcs = [];
  for (let i = 0; i < f2.length - 3; i++) {
    if (f2[i] === 0x1f && f2[i + 1] === 0x43 && f2[i + 2] === 0xb6 && f2[i + 3] === 0x75) {
      // skip the cluster's own size vint, then the first child is Timecode:
      // id e7 + 1-byte size vint + big-endian uint payload
      let p2 = i + 4;
      const b = f2[p2];
      // the marker bit's position gives the vint length (0x80|n → 1 byte,
      // 0x40.. → 2 bytes, …)
      let k = 0;
      for (let bit = 7; bit >= 0; bit--) {
        if (b & (1 << bit)) { k = 7 - bit; break; }
      }
      p2 += k + 1;
      if (f2[p2] === 0xe7) {
        const n = f2[p2 + 1] & 0x7f;
        let v = 0;
        for (let q = 0; q < n; q++) v = v * 256 + f2[p2 + 2 + q];
        tcs.push(v);
      }
    }
  }
  return JSON.stringify(tcs) === JSON.stringify([0, 20000, 40000]);
})());

console.log('\n[4] honesty guards');
assert.throws(() => new WebmMuxer({ width: 0, height: 2 }), /width\/height/);
assert.throws(() => new WebmMuxer({ width: 2 }), /width\/height/);
ok('the muxer refuses a track without real dimensions', true);
const m3 = new WebmMuxer({ width: 2, height: 2 });
m3.addBlock(new Uint8Array([1]), 500, true);
assert.throws(() => m3.addBlock(new Uint8Array([2]), 400, false), /non-decreasing/);
ok('timestamps that run backwards are refused (VFR is fine, reordering is not)', true);
m3.finish(500);
assert.throws(() => m3.addBlock(new Uint8Array([3]), 600, false), /finished/);
assert.throws(() => m3.finish(500), /finished/);
ok('a finished muxer accepts nothing more (no silent tail frames)', true);

console.log('\n[5] frames/bytes counters');
const m4 = new WebmMuxer({ width: 2, height: 2 });
m4.addBlock(new Uint8Array([1, 2, 3, 4]), 0, true);
m4.addBlock(new Uint8Array([5]), 33, false);
m4.finish(33);
ok('frames counts encoded blocks', m4.frames === 2);
ok('bytes counts encoded payload only (no container overhead)', m4.bytes === 5);
ok('durationMs reports what was written', m4.durationMs === 33);

console.log(`\n=== test-video: ${passed} passed, ${failed} failed ===`);
if (failed) {
  console.error('FAILED:', failures.join(' · '));
  process.exit(1);
}
