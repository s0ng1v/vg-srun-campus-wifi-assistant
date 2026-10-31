import { createHash } from "node:crypto";
import { readFileSync, readdirSync, statSync } from "node:fs";
import { dirname, relative, resolve, sep } from "node:path";
import { fileURLToPath } from "node:url";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const vendor = resolve(root, "src/vendor/lens-design-system");
const expected = "3d1d5496896f2cdffd76686429dd1ce7e5a6719957d87faf90d9029cbf501015";
const sourceExtensions = new Set([".css", ".d.ts", ".js", ".json", ".ts"]);

function collect(directory) {
  const files = [];
  for (const name of readdirSync(directory)) {
    const path = resolve(directory, name);
    if (statSync(path).isDirectory()) files.push(...collect(path));
    else if (sourceExtensions.has(name.slice(name.lastIndexOf("."))) || name.endsWith(".d.ts")) files.push(path);
  }
  return files;
}

const hash = createHash("sha256");
const files = collect(vendor).sort((left, right) => {
  const a = relative(vendor, left).split(sep).join("/");
  const b = relative(vendor, right).split(sep).join("/");
  return a.localeCompare(b, "en");
});

for (const file of files) {
  const name = relative(vendor, file).split(sep).join("/");
  hash.update(`${name}\0`, "utf8");
  hash.update(readFileSync(file));
}

const actual = hash.digest("hex");
if (actual !== expected) {
  console.error(`Lens vendor files changed. Expected ${expected}, received ${actual}.`);
  process.exitCode = 1;
} else {
  console.log(`Verified ${files.length} Lens source files from 7a424c93b9be.`);
}
