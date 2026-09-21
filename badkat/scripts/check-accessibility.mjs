import fs from "node:fs/promises";
import path from "node:path";
import { fileURLToPath } from "node:url";
import axe from "axe-core";
import { JSDOM } from "jsdom";

const here = path.dirname(fileURLToPath(import.meta.url));
const html = await fs.readFile(path.join(here, "..", "src", "settings.html"), "utf8");
const dom = new JSDOM(html, { runScripts: "outside-only", url: "https://badkat.local/" });
dom.window.matchMedia = () => ({ matches: false, addEventListener() {} });
const settings = await fs.readFile(path.join(here, "..", "src", "settings.js"), "utf8");
dom.window.eval(settings);
await new Promise((resolve) => setTimeout(resolve, 0));
if (!dom.window.document.querySelector("#ruleRows .rule")) {
  throw new Error("Settings did not render generated rule controls");
}
dom.window.eval(axe.source);

const failures = [];
for (const panel of dom.window.document.querySelectorAll(".panel")) {
  for (const other of dom.window.document.querySelectorAll(".panel")) {
    other.hidden = other !== panel;
  }
  const results = await dom.window.axe.run(dom.window.document, {
    rules: {
      // jsdom has no layout engine, so contrast is verified from the fixed
      // design tokens in review while axe owns semantic checks here.
      "color-contrast": { enabled: false }
    }
  });
  failures.push(...results.violations
    .filter((violation) => violation.impact === "critical" || violation.impact === "serious")
    .map((violation) => ({ panel: panel.id, ...violation })));
}

if (failures.length) {
  for (const failure of failures) {
    console.error(`${failure.panel}: ${failure.impact}: ${failure.id} — ${failure.help}`);
    for (const node of failure.nodes) {
      console.error(`  ${node.target.join(" ")}: ${node.failureSummary}`);
    }
  }
  process.exitCode = 1;
} else {
  console.log("Accessibility check passed: all rendered panels have no critical or serious axe violations.");
}
dom.window.close();
