import { test } from "node:test";
import assert from "node:assert/strict";
import { mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import { assemble, channel } from "./assemble-update-channels.mjs";

function fixture(version, platforms) {
  const signature = Buffer.from(`untrusted comment: fixture\nx\ntrusted comment: timestamp:0\tversion:${version}\nx`).toString("base64");
  const entry = { signature, url: `https://github.com/Mikuku-Kawaii/ASMRplayer/releases/download/v${version}/file` };
  return { version, platforms: Object.fromEntries(platforms.map(key => [key, entry])) };
}
const win = ["windows-x86_64", "windows-x86_64-nsis", "windows-x86_64-msi"];
const android = ["android-universal"];

test("bridge remains unchanged while the other platform retains its own version", async () => {
  const dir = await mkdtemp(path.join(os.tmpdir(), "asmr-channels-"));
  const originalFetch = globalThis.fetch;
  try {
    const bridge = JSON.stringify(fixture("26.11.12", [...win, ...android]));
    const previous = JSON.stringify(fixture("26.11.15", win));
    await writeFile(path.join(dir, "latest.json"), JSON.stringify(fixture("26.11.16", android)));
    globalThis.fetch = async url => ({ ok: true, text: async () => url.includes("/v26.11.12/") ? bridge : previous });
    await assemble(dir);
    assert.equal(await readFile(path.join(dir, "latest.json"), "utf8"), bridge);
    assert.equal(JSON.parse(await readFile(path.join(dir, "latest-windows.json"))).version, "26.11.15");
    assert.equal(JSON.parse(await readFile(path.join(dir, "latest-android.json"))).version, "26.11.16");
  } finally { globalThis.fetch = originalFetch; await rm(dir, { recursive: true, force: true }); }
});

test("rejects a manifest which presents a different signed version", () => {
  const data = fixture("26.11.11", win);
  data.version = "26.11.12";
  for (const value of Object.values(data.platforms)) value.url = value.url.replace("v26.11.11", "v26.11.12");
  assert.throws(() => channel(data, "windows"), /Signed version mismatch/);
});

test("a Windows-only release retains the previous Android channel", async () => {
  const dir = await mkdtemp(path.join(os.tmpdir(), "asmr-channels-"));
  const originalFetch = globalThis.fetch;
  try {
    const bridge = JSON.stringify(fixture("26.11.12", [...win, ...android]));
    const previous = JSON.stringify(fixture("26.11.14", android));
    await writeFile(path.join(dir, "latest.json"), JSON.stringify(fixture("26.11.17", win)));
    globalThis.fetch = async url => ({ ok: true, text: async () => url.includes("/v26.11.12/") ? bridge : previous });
    await assemble(dir);
    assert.equal(await readFile(path.join(dir, "latest.json"), "utf8"), bridge);
    assert.equal(JSON.parse(await readFile(path.join(dir, "latest-windows.json"))).version, "26.11.17");
    assert.equal(JSON.parse(await readFile(path.join(dir, "latest-android.json"))).version, "26.11.14");
  } finally { globalThis.fetch = originalFetch; await rm(dir, { recursive: true, force: true }); }
});
