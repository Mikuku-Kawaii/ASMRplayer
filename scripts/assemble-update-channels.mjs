import { readFile, writeFile } from "node:fs/promises";
import path from "node:path";
import { pathToFileURL } from "node:url";

const repository = "https://github.com/Mikuku-Kawaii/ASMRplayer/releases";
const bridgeVersion = "26.11.12";
const windowsKeys = ["windows-x86_64", "windows-x86_64-nsis", "windows-x86_64-msi"];

export function channel(manifest, platform) {
  if (!/^\d+\.\d+\.\d+$/.test(manifest.version)) throw new Error("Invalid channel version");
  const keys = platform === "windows" ? windowsKeys : ["android-universal"];
  const selected = {};
  for (const key of keys) {
    const entry = manifest.platforms?.[key];
    const prefix = `${repository}/download/v${manifest.version}/`;
    if (!entry || typeof entry.signature !== "string" || !entry.signature ||
        typeof entry.url !== "string" || !entry.url.startsWith(prefix)) throw new Error(`Invalid ${key} channel entry`);
    const lines = Buffer.from(entry.signature, "base64").toString("utf8").split(/\r?\n/);
    const comment = lines.find(line => line.startsWith("trusted comment: ")) ?? "";
    if (!comment.split("\t").includes(`version:${manifest.version}`)) throw new Error(`Signed version mismatch for ${key}`);
    selected[key] = entry;
  }
  return { ...manifest, platforms: selected };
}

async function remote(name, pinned = false) {
  const url = pinned ? `${repository}/download/v${bridgeVersion}/${name}` : `${repository}/latest/download/${name}`;
  const response = await fetch(url);
  if (!response.ok) throw new Error(`Cannot retrieve ${name}: ${response.status}`);
  return await response.text();
}

export async function assemble(output) {
  const currentText = await readFile(path.join(output, "latest.json"), "utf8");
  const current = JSON.parse(currentText);
  const bridgeText = current.version === bridgeVersion ? currentText : await remote("latest.json", true);
  const bridge = JSON.parse(bridgeText);
  if (bridge.version !== bridgeVersion) throw new Error("Bridge version must remain 26.11.12");
  channel(bridge, "windows");
  channel(bridge, "android");
  const readChannel = async (platform, key) => {
    const source = current.platforms?.[key] ? current : JSON.parse(await remote(`latest-${platform}.json`));
    return channel(source, platform);
  };
  const windows = await readChannel("windows", "windows-x86_64");
  const android = await readChannel("android", "android-universal");
  await writeFile(path.join(output, "latest-windows.json"), JSON.stringify(windows, null, 2) + "\n");
  await writeFile(path.join(output, "latest-android.json"), JSON.stringify(android, null, 2) + "\n");
  await writeFile(path.join(output, "latest.json"), bridgeText);
  console.log(`Prepared bridge ${bridge.version}, Windows ${windows.version}, Android ${android.version}`);
}

if (process.argv[1] && import.meta.url === pathToFileURL(path.resolve(process.argv[1])).href) {
  if (!process.argv[2]) throw new Error("Usage: node scripts/assemble-update-channels.mjs <release-directory>");
  await assemble(process.argv[2]);
}
