import { createHash } from "node:crypto";
import { verifyUpdaterSignature } from "./verify-updater-signature.mjs";

// Shared by both release entry points. The Android release script checks APK metadata/certificates first.
export function addAndroidUpdate(manifest, bytes, signature, publicKey, metadata, assetName) {
  const { version, versionCode, minSdk, packageName } = metadata;
  if (!/^\d+\.\d+\.\d+$/.test(version) || manifest.version !== version
      || packageName !== "com.asmrplayer.android" || !Number.isInteger(versionCode)
      || versionCode < 1 || versionCode > 2100000000 || !Number.isInteger(minSdk) || minSdk < 23
      || !/^[A-Za-z0-9._-]+\.apk$/.test(assetName) || !bytes.length || bytes.length > 512 * 1024 * 1024) {
    throw new Error("Android package metadata does not match the shared stable release");
  }
  const packet = Buffer.from(Buffer.from(signature.trim(), "base64").toString("utf8").trim().split(/\r?\n/)[1] ?? "", "base64");
  if (packet.subarray(0, 2).toString() !== "ED") throw new Error("Android requires a prehashed Minisign signature");
  verifyUpdaterSignature(bytes, signature, publicKey, version);
  return { ...manifest, platforms: { ...manifest.platforms, "android-universal": {
    url: `https://github.com/Mikuku-Kawaii/ASMRplayer/releases/download/v${version}/${assetName}`,
    signature: signature.trim(), versionCode, minSdk, size: bytes.length,
    sha256: createHash("sha256").update(bytes).digest("hex"),
  } } };
}
