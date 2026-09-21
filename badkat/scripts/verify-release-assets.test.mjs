import assert from "node:assert/strict";
import fs from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import { verifyReleaseAssets } from "./verify-release-assets.mjs";

const dir = await fs.mkdtemp(path.join(os.tmpdir(), "badkat-release-test-"));
try {
  await Promise.all([
    fs.writeFile(path.join(dir, "BadKat_0.1.1_x64-setup.exe"), "installer"),
    fs.writeFile(path.join(dir, "BadKat_0.1.1_x64_en-US.msi"), "installer"),
    fs.writeFile(path.join(dir, "BadKat_0.1.1_x64-setup.exe.sig"), "signed"),
    fs.writeFile(
      path.join(dir, "latest.json"),
      JSON.stringify({
        version: "0.1.1",
        platforms: {
          "windows-x86_64": { url: "https://example.invalid/v0.1.1/BadKat_0.1.1_x64-setup.exe", signature: "signed" }
        }
      })
    )
  ]);

  const assets = await verifyReleaseAssets(dir, "0.1.1");
  assert.equal(assets.exe, "BadKat_0.1.1_x64-setup.exe");
  await assert.rejects(() => verifyReleaseAssets(dir, "9.9.9"), /Missing release assets/);
  const latestPath = path.join(dir, "latest.json");
  const latest = JSON.parse(await fs.readFile(latestPath, "utf8"));
  latest.platforms["windows-x86_64"].url = "https://example.invalid/v0.1.0/BadKat_0.1.0_x64-setup.exe";
  await fs.writeFile(latestPath, JSON.stringify(latest));
  await assert.rejects(() => verifyReleaseAssets(dir, "0.1.1"), /versioned, signed/);
  latest.platforms["windows-x86_64"].url = "https://example.invalid/v0.1.1/BadKat_0.1.1_x64-setup.exe";
  latest.platforms["windows-x86_64"].url = "https://example.invalid/v0.1.0/BadKat_0.1.1_x64-setup.exe";
  await fs.writeFile(latestPath, JSON.stringify(latest));
  await assert.rejects(() => verifyReleaseAssets(dir, "0.1.1"), /versioned, signed/);
  latest.platforms["windows-x86_64"].url = "https://example.invalid/v0.1.1/BadKat_0.1.1_x64-setup.exe";
  latest.platforms["windows-x86_64"].signature = "wrong";
  await fs.writeFile(latestPath, JSON.stringify(latest));
  await assert.rejects(() => verifyReleaseAssets(dir, "0.1.1"), /signature does not match/);
  latest.platforms["windows-x86_64"].signature = "signed";
  await fs.writeFile(latestPath, JSON.stringify(latest));
  await fs.rm(path.join(dir, "BadKat_0.1.1_x64-setup.exe.sig"));
  await assert.rejects(() => verifyReleaseAssets(dir, "0.1.1"), /versioned, signed/);
  console.log("Release asset verifier tests passed.");
} finally {
  await fs.rm(dir, { recursive: true, force: true });
}
