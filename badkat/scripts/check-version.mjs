import fs from "node:fs/promises";
import path from "node:path";
import { fileURLToPath } from "node:url";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const packageJson = JSON.parse(await fs.readFile(path.join(root, "package.json"), "utf8"));
const tauriJson = JSON.parse(await fs.readFile(path.join(root, "src-tauri", "tauri.conf.json"), "utf8"));
const cargo = await fs.readFile(path.join(root, "src-tauri", "Cargo.toml"), "utf8");
const cargoVersion = /^version\s*=\s*"([^"]+)"/m.exec(cargo)?.[1];
const versions = [packageJson.version, tauriJson.version, cargoVersion];

if (!cargoVersion || new Set(versions).size !== 1) {
  throw new Error(`Version mismatch: package=${versions[0]}, tauri=${versions[1]}, cargo=${versions[2]}`);
}

const tag = process.argv[2] || process.env.GITHUB_REF_NAME || "";
if (tag && tag !== `v${packageJson.version}`) {
  throw new Error(`Tag ${tag} does not match v${packageJson.version}`);
}
console.log(`Version check passed: ${packageJson.version}${tag ? ` (${tag})` : ""}`);
