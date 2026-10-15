import { readFile, writeFile, copyFile, mkdir } from "node:fs/promises";
import { createHash } from "node:crypto";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { verifyUpdaterSignature } from "./verify-updater-signature.mjs";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const pkg = JSON.parse(await readFile(path.join(root, "package.json"), "utf8"));
const config = JSON.parse(await readFile(path.join(root, "src-tauri/tauri.conf.json"), "utf8"));
const cargo = await readFile(path.join(root, "src-tauri/Cargo.toml"), "utf8");
const version = pkg.version;
const rustVersion = cargo.match(/\[package\][\s\S]*?^version\s*=\s*"([^"]+)"/m)?.[1];
if (!/^\d+\.\d+\.\d+$/.test(version) || config.version !== version || rustVersion !== version) {
  throw new Error("Use matching stable versions in package.json, Cargo.toml and tauri.conf.json");
}
const tag = `v${version}`;
if (process.env.GITHUB_REF_TYPE === "tag" && process.env.GITHUB_REF_NAME !== tag) throw new Error("Release tag does not match application version");
const output = path.join(root, "output/release", tag);
await mkdir(output, { recursive: true });
const platforms = {};
const hashes = [];
for (const [type, filename] of [
  ["nsis", `ASMR Player_${version}_x64-setup.exe`],
  ["msi", `ASMR Player_${version}_x64_en-US.msi`],
]) {
  const source = path.join(root, "src-tauri/target/release/bundle", type, filename);
  const signature = (await readFile(`${source}.sig`, "utf8")).trim();
  if (!signature) throw new Error(`Missing signature: ${source}`);
  const bytes = await readFile(source);
  verifyUpdaterSignature(bytes, signature, config.plugins.updater.pubkey, version);
  // Use GitHub-safe names so upload normalization cannot break manifest URLs.
  const assetName = filename.replaceAll(" ", ".");
  await copyFile(source, path.join(output, assetName));
  await copyFile(`${source}.sig`, path.join(output, `${assetName}.sig`));
  const entry = { signature, url: `https://github.com/Mikuku-Kawaii/ASMRplayer/releases/download/${tag}/${encodeURIComponent(assetName)}` };
  platforms[`windows-x86_64-${type}`] = entry;
  if (type === "nsis") platforms["windows-x86_64"] = entry;
  hashes.push(`${createHash("sha256").update(bytes).digest("hex")}  ${assetName}`);
}
const notes = await readFile(path.join(root, "docs/releases", `${tag}.md`), "utf8");
await writeFile(path.join(output, "latest.json"), JSON.stringify({ version, notes, pub_date: new Date().toISOString(), platforms }, null, 2) + "\n");
await writeFile(path.join(output, "SHA256SUMS.txt"), hashes.join("\n") + "\n");
try {
  await copyFile(path.join(root, "THIRD_PARTY_NOTICES.md"), path.join(output, "THIRD_PARTY_NOTICES.md"));
} catch (error) {
  if (error.code !== "ENOENT") throw error;
}
console.log(`Signed release files and latest.json prepared in ${output}`);
