import { createHash, createPublicKey, verify } from "node:crypto";

// Tauri wraps a Minisign document in base64. Verify both the artifact signature
// and its trusted version comment, using the public key embedded in the app.
export function verifyUpdaterSignature(bytes, encodedSignature, encodedPublicKey, version) {
  const keyLines = Buffer.from(encodedPublicKey.trim(), "base64").toString("utf8").trim().split(/\r?\n/);
  const lines = Buffer.from(encodedSignature.trim(), "base64").toString("utf8").trim().split(/\r?\n/);
  const keyPacket = Buffer.from(keyLines[1] ?? "", "base64");
  const packet = Buffer.from(lines[1] ?? "", "base64");
  if (keyPacket.length !== 42 || keyPacket.subarray(0, 2).toString() !== "Ed"
      || lines.length !== 4 || packet.length !== 74 || !lines[2].startsWith("trusted comment: ")) {
    throw new Error("Invalid Tauri/Minisign signature encoding");
  }
  if (!keyPacket.subarray(2, 10).equals(packet.subarray(2, 10))) {
    throw new Error("Updater signature uses a different signing key");
  }
  const key = createPublicKey({ format: "der", type: "spki", key: Buffer.concat([
    Buffer.from("302a300506032b6570032100", "hex"), keyPacket.subarray(10),
  ]) });
  const algorithm = packet.subarray(0, 2).toString();
  if (algorithm !== "ED" && algorithm !== "Ed") throw new Error("Unsupported Minisign algorithm");
  const payload = algorithm === "ED" ? createHash("blake2b512").update(bytes).digest() : bytes;
  const signature = packet.subarray(10);
  if (!verify(null, payload, key, signature)) throw new Error("Installer signature verification failed");
  const comment = lines[2].slice("trusted comment: ".length);
  if (!verify(null, Buffer.concat([signature, Buffer.from(comment)]), key, Buffer.from(lines[3], "base64"))) {
    throw new Error("Signature trusted comment verification failed");
  }
  const versions = comment.split("\t").filter(field => field.startsWith("version:"));
  if (versions.length !== 1 || versions[0] !== `version:${version}`) {
    throw new Error("Signed installer version does not match the release version");
  }
}
