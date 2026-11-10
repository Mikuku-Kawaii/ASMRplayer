import test from "node:test";
import assert from "node:assert/strict";
import { generateKeyPairSync, sign, createHash } from "node:crypto";
import { addAndroidUpdate } from "./android-update-manifest.mjs";

const { publicKey, privateKey } = generateKeyPairSync("ed25519");
const id = Buffer.from("12345678");
const b64 = bytes => Buffer.from(bytes).toString("base64");
const key = b64(`untrusted comment: test\n${b64(Buffer.concat([Buffer.from("Ed"), id, publicKey.export({ format: "der", type: "spki" }).subarray(-32)]))}\n`);
const bytes = Buffer.from("synthetic APK signature fixture");
const raw = sign(null, createHash("blake2b512").update(bytes).digest(), privateKey);
const comment = "timestamp:1\tversion:1.4.0";
const signature = b64(`untrusted comment: fixture\n${b64(Buffer.concat([Buffer.from("ED"), id, raw]))}\ntrusted comment: ${comment}\n${b64(sign(null, Buffer.concat([raw, Buffer.from(comment)]), privateKey))}\n`);
const manifest = { version: "1.4.0", notes: "Release", platforms: { "windows-x86_64": { signature: "existing", url: "existing.exe" } } };
const metadata = { version: "1.4.0", versionCode: 14, minSdk: 23, packageName: "com.asmrplayer.android" };
test("merges the signed APK while preserving PC entries and release notes", () => {
  const result = addAndroidUpdate(manifest, bytes, signature, key, metadata, "release.apk");
  assert.deepEqual(result.platforms["windows-x86_64"], manifest.platforms["windows-x86_64"]);
  assert.equal(result.notes, manifest.notes);
  assert.equal(result.platforms["android-universal"].versionCode, 14);
  assert.equal(result.platforms["android-universal"].size, bytes.length);
  assert.equal(manifest.platforms["android-universal"], undefined);
});
test("rejects modified artifacts and version substitution", () => {
  assert.throws(() => addAndroidUpdate(manifest, Buffer.from("tampered"), signature, key, metadata, "release.apk"));
  assert.throws(() => addAndroidUpdate({ ...manifest, version: "9.9.9" }, bytes, signature, key, { ...metadata, version: "9.9.9" }, "release.apk"));
});
test("rejects wrong package, invalid Android codes and unsafe asset paths", () => {
  for (const change of [{ packageName: "wrong.package" }, { versionCode: 0 }, { minSdk: 22 }]) {
    assert.throws(() => addAndroidUpdate(manifest, bytes, signature, key, { ...metadata, ...change }, "release.apk"));
  }
  assert.throws(() => addAndroidUpdate(manifest, bytes, signature, key, metadata, "../release.apk"));
});
