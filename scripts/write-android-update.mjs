import { readFile, writeFile, copyFile, mkdir } from "node:fs/promises";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { addAndroidUpdate } from "./android-update-manifest.mjs";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const [apk, metadataPath] = process.argv.slice(2);
if (!apk || !metadataPath) throw new Error("Usage: node scripts/write-android-update.mjs <signed.apk> <verified-metadata.json>");
const config = JSON.parse(await readFile(path.join(root, "src-tauri/tauri.conf.json"), "utf8"));
const metadata = JSON.parse(await readFile(metadataPath, "utf8"));
const version = JSON.parse(await readFile(path.join(root, "package.json"), "utf8")).version;
if (version !== metadata.version || version !== config.version) throw new Error("Release versions differ");
const output = path.join(root, "output/release", `v${version}`);
await mkdir(output, { recursive: true });
const manifestPath = path.join(output, "latest.json");
let manifest;
try { manifest = JSON.parse(await readFile(manifestPath, "utf8")); }
catch (error) {
  if (error.code !== "ENOENT") throw error;
  manifest = { version, notes: await readFile(path.join(root, "docs/releases", `v${version}.md`), "utf8"), pub_date: new Date().toISOString(), platforms: {} };
}
const assetName = `ASMR.Player_${version}_${metadata.versionCode}_universal.apk`;
const bytes = await readFile(apk);
const signature = (await readFile(`${apk}.sig`, "utf8")).trim();
const next = addAndroidUpdate(manifest, bytes, signature, config.plugins.updater.pubkey, metadata, assetName);
await copyFile(apk, path.join(output, assetName));
await copyFile(`${apk}.sig`, path.join(output, `${assetName}.sig`));
// Retain metadata so a subsequent PC build can re-validate and retain the Android entry.
await writeFile(path.join(output, "android-release.json"), JSON.stringify({ ...metadata, assetName }, null, 2) + "\n");
await writeFile(manifestPath, JSON.stringify(next, null, 2) + "\n");
let hashes = "";
try { hashes = await readFile(path.join(output, "SHA256SUMS.txt"), "utf8"); }
catch (error) { if (error.code !== "ENOENT") throw error; }
const lines = hashes.split(/\r?\n/).filter(line => line && !line.endsWith(".apk"));
lines.push(`${next.platforms["android-universal"].sha256}  ${assetName}`);
await writeFile(path.join(output, "SHA256SUMS.txt"), lines.join("\n") + "\n");
console.log(`Android APK merged into ${manifestPath}; existing Windows entries retained.`);
