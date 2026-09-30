import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { mkdtempSync, mkdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test } from "node:test";
import { fileURLToPath } from "node:url";

const script = fileURLToPath(new URL("./inline-dist.mjs", import.meta.url));

function fixture(html) {
  const dir = mkdtempSync(join(tmpdir(), "wallpaper-inline-"));
  mkdirSync(join(dir, "assets"));
  writeFileSync(join(dir, "index.html"), html);
  writeFileSync(join(dir, "assets", "app.css"), "body { color: red; }");
  writeFileSync(join(dir, "assets", "app.js"), "document.getElementById('app').textContent = 'ready';");
  return dir;
}

test("inline bundle runs after the app root and has no external assets", () => {
  const dir = fixture('<html><head><script type="module" src="./assets/app.js"></script><link rel="stylesheet" href="./assets/app.css"></head><body><div id="app"></div></body></html>');
  try {
    execFileSync(process.execPath, [script], { env: { ...process.env, DW_DIST_DIR: dir } });
    const html = readFileSync(join(dir, "index.html"), "utf8");
    assert.match(html, /<style data-vite-inline>body \{ color: red; \}<\/style>/);
    assert.ok(html.indexOf('<div id="app"></div>') < html.indexOf("<script data-vite-inline>"));
    assert.ok(html.indexOf("<script data-vite-inline>") < html.indexOf("</body>"));
    assert.doesNotMatch(html, /src="\.\/assets\//);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});

test("missing app root fails build instead of shipping a blank window", () => {
  const dir = fixture('<html><head><script type="module" src="./assets/app.js"></script><link href="./assets/app.css"></head><body></body></html>');
  try {
    assert.throws(() => execFileSync(process.execPath, [script], {
      env: { ...process.env, DW_DIST_DIR: dir }, stdio: "pipe",
    }), /Command failed/);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});
