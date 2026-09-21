import fs from "node:fs/promises";
import path from "node:path";
import { pathToFileURL } from "node:url";

export async function verifyReleaseAssets(directory, version) {
  const names = await fs.readdir(directory);
  const versioned = (extension) => names.find((name) =>
    name.includes(version) && name.toLowerCase().endsWith(extension)
  );
  const exe = versioned(".exe");
  const msi = versioned(".msi");
  if (!names.includes("latest.json") || !exe || !msi) {
    throw new Error(`Missing release assets for ${version}: latest.json, NSIS .exe, and MSI are required`);
  }

  const latest = JSON.parse(await fs.readFile(path.join(directory, "latest.json"), "utf8"));
  if (latest.version !== version) {
    throw new Error(`latest.json version ${latest.version} does not match ${version}`);
  }
  const platforms = Object.entries(latest.platforms || {});
  if (!platforms.length || !platforms.some(([platform]) => platform.startsWith("windows-"))) {
    throw new Error("latest.json must contain a Windows platform");
  }
  for (const [platform, item] of platforms) {
    if (!item.url || !item.signature?.trim()) {
      throw new Error(`${platform} must contain a URL and updater signature`);
    }
    const url = new URL(item.url);
    const asset = decodeURIComponent(url.pathname.split("/").pop());
    if (!url.pathname.includes(`/v${version}/`) || !asset.includes(version)
        || !names.includes(asset) || !names.includes(`${asset}.sig`)) {
      throw new Error(`${platform} must reference a versioned, signed release asset`);
    }
    const signature = (await fs.readFile(path.join(directory, `${asset}.sig`), "utf8")).trim();
    if (signature !== item.signature.trim()) {
      throw new Error(`${platform} signature does not match ${asset}.sig`);
    }
  }
  return { exe, msi };
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  const [directory, version] = process.argv.slice(2);
  if (!directory || !version) {
    throw new Error("Usage: verify-release-assets.mjs <directory> <version>");
  }
  const assets = await verifyReleaseAssets(directory, version);
  console.log(`Release assets verified for ${version}: ${assets.exe}, ${assets.msi}`);
}
