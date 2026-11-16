// Line-level diff for snapshot text (LCS on lines, capped).
// Snapshot diffing (the "read what changed after the action" pattern): the
// previous capture's text is kept per tab, and a diff request returns only the
// lines that appeared/disappeared instead of re-reading the whole tree.
//
// The LCS here is O(n·m) on lines, so both inputs are hard-capped: beyond the
// cap the diff degrades honestly (diff_truncated:true, only the first N lines
// are compared) rather than burning the service worker on a quadratic walk.

export const DIFF_MAX_LINES = 1200;
export const DIFF_MAX_OUT_LINES = 200;

function splitLines(text) {
  return String(text || '').split('\n');
}

/**
 * Diff two texts line by line.
 * Returns { added, removed, same, trunc } where added/removed are arrays of
 * [lineNoInNewOrOld, lineText] and same is the count of unchanged lines.
 * `trunc` reports whether the line caps degraded the comparison.
 * `keyOf` (optional) compares lines by a normalized key while still REPORTING
 * the original lines — snapshot diffing uses it to strip the @eN ref numbers
 * (every snapshot re-registers refs with fresh numbers, so a raw line compare
 * would flag a byte-identical page as 100% changed).
 */
export function diffLines(oldText, newText, { maxOut = DIFF_MAX_OUT_LINES, keyOf = null } = {}) {
  const a = splitLines(oldText);
  const b = splitLines(newText);
  const key = keyOf || ((l) => l);
  let trunc = false;
  if (a.length > DIFF_MAX_LINES) {
    a.length = DIFF_MAX_LINES;
    trunc = true;
  }
  if (b.length > DIFF_MAX_LINES) {
    b.length = DIFF_MAX_LINES;
    trunc = true;
  }
  // LCS table (rows = old lines, cols = new lines). The cap keeps the worst
  // case at ~1.4M cells of small ints, which is fine for a service worker.
  const n = a.length;
  const m = b.length;
  const ka = a.map(key);
  const kb = b.map(key);
  // dp[i][j] = LCS length of a[i..] and b[j..]; store one row at a time
  const dp = new Uint32Array((n + 1) * (m + 1));
  const W = m + 1;
  for (let i = n - 1; i >= 0; i--) {
    const ai = ka[i];
    for (let j = m - 1; j >= 0; j--) {
      dp[i * W + j] = ai === kb[j] ? dp[(i + 1) * W + j + 1] + 1 : Math.max(dp[(i + 1) * W + j], dp[i * W + j + 1]);
    }
  }
  const added = [];
  const removed = [];
  let same = 0;
  let i = 0;
  let j = 0;
  while (i < n && j < m) {
    if (ka[i] === kb[j]) {
      same++;
      i++;
      j++;
    } else if (dp[(i + 1) * W + j] >= dp[i * W + j + 1]) {
      if (removed.length < maxOut) removed.push([i + 1, a[i]]);
      i++;
    } else {
      if (added.length < maxOut) added.push([j + 1, b[j]]);
      j++;
    }
  }
  for (; i < n; i++) if (removed.length < maxOut) removed.push([i + 1, a[i]]);
  for (; j < m; j++) if (added.length < maxOut) added.push([j + 1, b[j]]);
  return {
    added,
    removed,
    same,
    trunc: trunc || added.length >= maxOut || removed.length >= maxOut,
  };
}

/** Format a diff result as compact text the model reads directly. */
export function formatDiff(d) {
  const parts = [];
  if (d.added.length) {
    parts.push(`+ ${d.added.length} added:`);
    for (const [no, line] of d.added) parts.push(`  +${no} ${line}`);
  }
  if (d.removed.length) {
    parts.push(`- ${d.removed.length} removed:`);
    for (const [no, line] of d.removed) parts.push(`  -${no} ${line}`);
  }
  if (!parts.length) parts.push('(no changed lines)');
  if (d.trunc) parts.push('(diff truncated — larger than the line budget)');
  return parts.join('\n');
}
