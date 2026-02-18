import { toInt, splitLines } from "./common.js";

function lcsTable(a, b) {
  const n = a.length;
  const m = b.length;
  const dp = Array.from({ length: n + 1 }, () => new Array(m + 1).fill(0));
  for (let i = n - 1; i >= 0; i -= 1) {
    for (let j = m - 1; j >= 0; j -= 1) {
      dp[i][j] = a[i] === b[j] ? (dp[i + 1][j + 1] + 1) : Math.max(dp[i + 1][j], dp[i][j + 1]);
    }
  }
  return dp;
}

function backtrackDiff(a, b, dp) {
  const ops = [];
  let i = 0;
  let j = 0;
  while (i < a.length && j < b.length) {
    if (a[i] === b[j]) {
      ops.push({ type: "equal", line: a[i] });
      i += 1;
      j += 1;
    } else if (dp[i + 1][j] >= dp[i][j + 1]) {
      ops.push({ type: "del", line: a[i] });
      i += 1;
    } else {
      ops.push({ type: "add", line: b[j] });
      j += 1;
    }
  }
  while (i < a.length) {
    ops.push({ type: "del", line: a[i] });
    i += 1;
  }
  while (j < b.length) {
    ops.push({ type: "add", line: b[j] });
    j += 1;
  }
  return ops;
}

function buildHunks(ops, contextLines) {
  const ctx = Math.max(0, toInt(contextLines, 3));
  const hunks = [];

  let oldLine = 1;
  let newLine = 1;
  const indexed = ops.map((op) => {
    const item = { ...op, oldLine, newLine };
    if (op.type === "equal") {
      oldLine += 1;
      newLine += 1;
    }
    if (op.type === "del") oldLine += 1;
    if (op.type === "add") newLine += 1;
    return item;
  });

  const changeIdx = [];
  for (let k = 0; k < indexed.length; k += 1) {
    if (indexed[k].type !== "equal") changeIdx.push(k);
  }
  if (changeIdx.length === 0) return hunks;

  let start = changeIdx[0];
  let end = changeIdx[0];

  function pushBlock(s, e) {
    const blockStart = Math.max(0, s - ctx);
    const blockEnd = Math.min(indexed.length - 1, e + ctx);

    let oldStart = null;
    let newStart = null;
    let oldCount = 0;
    let newCount = 0;

    const lines = [];
    for (let k = blockStart; k <= blockEnd; k += 1) {
      const op = indexed[k];
      if (oldStart === null) oldStart = op.oldLine;
      if (newStart === null) newStart = op.newLine;

      if (op.type === "equal") {
        lines.push(` ${op.line}`);
        oldCount += 1;
        newCount += 1;
      } else if (op.type === "del") {
        lines.push(`-${op.line}`);
        oldCount += 1;
      } else if (op.type === "add") {
        lines.push(`+${op.line}`);
        newCount += 1;
      }
    }

    if (oldStart === null) oldStart = 1;
    if (newStart === null) newStart = 1;

    hunks.push({ oldStart, oldCount, newStart, newCount, lines });
  }

  for (let idx = 1; idx < changeIdx.length; idx += 1) {
    const k = changeIdx[idx];
    if (k <= end + (ctx * 2) + 1) {
      end = k;
    } else {
      pushBlock(start, end);
      start = k;
      end = k;
    }
  }
  pushBlock(start, end);

  return hunks;
}

export function formatUnifiedDiffFile(relPath, oldText, newText, oldExists, newExists) {
  if (oldText === newText) return "";

  const oldLines = splitLines(oldText);
  const newLines = splitLines(newText);

  const oldHeader = oldExists ? `a/${relPath}` : "/dev/null";
  const newHeader = newExists ? `b/${relPath}` : "/dev/null";

  const dp = lcsTable(oldLines, newLines);
  const ops = backtrackDiff(oldLines, newLines, dp);
  const hunks = buildHunks(ops, 3);

  if (hunks.length === 0) {
    const lines = [];
    lines.push(`--- ${oldHeader}`);
    lines.push(`+++ ${newHeader}`);
    lines.push(`@@ -1,${oldLines.length} +1,${newLines.length} @@`);
    for (let i = 0; i < oldLines.length; i += 1) lines.push(`-${oldLines[i]}`);
    for (let i = 0; i < newLines.length; i += 1) lines.push(`+${newLines[i]}`);
    return `${lines.join("\n")}\n`;
  }

  const out = [];
  out.push(`diff --git a/${relPath} b/${relPath}`);
  out.push(`--- ${oldHeader}`);
  out.push(`+++ ${newHeader}`);

  for (let h = 0; h < hunks.length; h += 1) {
    const hk = hunks[h];
    out.push(`@@ -${hk.oldStart},${hk.oldCount} +${hk.newStart},${hk.newCount} @@`);
    out.push(...hk.lines);
  }

  return `${out.join("\n")}\n`;
}
