#!/usr/bin/env node
// Fails when a production Rust function exceeds MAX_LINES (default 80).
//
// Test code is exempt: functions annotated #[test]/#[tokio::test] and
// everything inside a #[cfg(test)] module are skipped. The dev-only
// service-check binary is exempt too.
import { readdirSync, readFileSync, statSync } from "node:fs";
import { join } from "node:path";

const MAX = Number(process.env.MAX_FN_LINES ?? 80);
const roots = process.argv.slice(2);
if (roots.length === 0) roots.push("crates", "src-tauri/src");

function walk(dir, out = []) {
  for (const entry of readdirSync(dir)) {
    const path = join(dir, entry);
    if (statSync(path).isDirectory()) walk(path, out);
    else if (path.endsWith(".rs")) out.push(path);
  }
  return out;
}

// Removes string literals, char literals and comments, keeping the line count.
function clean(lines) {
  const out = [];
  let block = false;
  for (const line of lines) {
    let kept = "";
    let i = 0;
    while (i < line.length) {
      if (block) {
        const end = line.indexOf("*/", i);
        if (end < 0) { i = line.length; continue; }
        i = end + 2;
        block = false;
        continue;
      }
      if (line.startsWith("//", i)) break;
      if (line.startsWith("/*", i)) { block = true; i += 2; continue; }
      const c = line[i];
      if (c === "r" && line[i + 1] === "#") {
        const match = /^r(#+")/.exec(line.slice(i));
        if (match) {
          const end = line.indexOf(match[1], i + match[0].length);
          i = end < 0 ? line.length : end + match[1].length;
          continue;
        }
      }
      if (c === '"') {
        i += 1;
        while (i < line.length) {
          if (line[i] === "\\") i += 2;
          else if (line[i] === '"') { i += 1; break; }
          else i += 1;
        }
        continue;
      }
      if (c === "'") {
        const end = line.indexOf("'", i + 1);
        if (end > i && end - i <= 4) { i = end + 1; continue; }
        i += 1;
        continue;
      }
      kept += c;
      i += 1;
    }
    out.push(kept);
  }
  return out;
}

// Lines inside a #[cfg(test)] module, which is all test code.
function testModuleLines(lines) {
  const skip = new Set();
  for (let i = 0; i < lines.length; i += 1) {
    if (!/^\s*#\[cfg\(test\)\]/.test(lines[i])) continue;
    let open = -1;
    for (let j = i + 1; j < Math.min(i + 4, lines.length); j += 1) {
      if (/^\s*(pub\s+)?mod\s+\w+\s*\{/.test(lines[j])) { open = j; break; }
    }
    if (open < 0) continue;
    let depth = 0;
    for (let j = open; j < lines.length; j += 1) {
      for (const ch of lines[j]) {
        if (ch === "{") depth += 1;
        else if (ch === "}") {
          depth -= 1;
          if (depth === 0) {
            for (let k = i; k <= j; k += 1) skip.add(k);
            j = lines.length;
            break;
          }
        }
      }
    }
  }
  return skip;
}

const violations = [];
for (const file of roots.flatMap((root) => walk(root))) {
  if (file.endsWith("bin/service-check.rs")) continue;
  const raw = readFileSync(file, "utf8").split("\n");
  const lines = clean(raw);
  const skip = testModuleLines(raw);
  for (let i = 0; i < lines.length; i += 1) {
    const match = /^\s*(?:pub(?:\([^)]*\))?\s+)?(?:const\s+)?(?:async\s+)?(?:unsafe\s+)?(?:extern\s+"[^"]*"\s+)?fn\s+(\w+)/.exec(lines[i]);
    if (!match || skip.has(i)) continue;
    const attrs = raw.slice(Math.max(0, i - 8), i).join("\n");
    if (/#\[(tokio::)?test\]/.test(attrs)) continue;
    let openLine = -1;
    let openCol = 0;
    for (let j = i; j < Math.min(i + 6, lines.length); j += 1) {
      const col = lines[j].indexOf("{");
      if (col >= 0) { openLine = j; openCol = col; break; }
      if (lines[j].includes(";")) break;
    }
    if (openLine < 0) continue;
    let depth = 0;
    let end = -1;
    for (let j = openLine; j < lines.length && end < 0; j += 1) {
      for (let k = j === openLine ? openCol : 0; k < lines[j].length; k += 1) {
        const ch = lines[j][k];
        if (ch === "{") depth += 1;
        else if (ch === "}") {
          depth -= 1;
          if (depth === 0) { end = j; break; }
        }
      }
    }
    if (end >= 0 && end - openLine + 1 > MAX) {
      violations.push({ file, line: i + 1, name: match[1], lines: end - openLine + 1 });
    }
  }
}

violations.sort((a, b) => b.lines - a.lines);
for (const v of violations) {
  console.log(`${v.file}:${v.line}  fn ${v.name}  ${v.lines} lines`);
}
console.log(`\n${violations.length} Rust function(s) over ${MAX} lines`);
process.exit(violations.length === 0 ? 0 : 1);
