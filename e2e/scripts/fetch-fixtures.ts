// Playwright globalSetup. Ensures the E2E fixture image exists before any
// test runs. We reuse the existing `src-tauri/tests/fixtures/page1.jpg`
// (a Catan-style rules page already used by the cargo integration suite)
// rather than depending on a third-party CDN. This keeps the suite hermetic.
//
// Idempotent: skips if the destination already exists with non-zero size.

import { copyFileSync, existsSync, mkdirSync, statSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const HERE = dirname(fileURLToPath(import.meta.url));
const ROOT = resolve(HERE, "..", "..");
const SRC = resolve(ROOT, "src-tauri/tests/fixtures/page1.jpg");
const DST = resolve(ROOT, "e2e/fixtures/catan-zh-page.jpg");

export default async function globalSetup(): Promise<void> {
  if (existsSync(DST) && statSync(DST).size > 0) {
    console.log(`[fixtures] reusing ${DST}`);
    return;
  }
  if (!existsSync(SRC)) {
    throw new Error(
      `fixture source missing: ${SRC}. ` +
        `Run the cargo integration tests once to generate the seed image.`,
    );
  }
  mkdirSync(dirname(DST), { recursive: true });
  copyFileSync(SRC, DST);
  console.log(`[fixtures] copied ${SRC} -> ${DST}`);
}

// Allow `tsx e2e/scripts/fetch-fixtures.ts` invocation from the CLI as well.
if (
  import.meta.url === `file://${process.argv[1]}` ||
  process.argv[1]?.endsWith("fetch-fixtures.ts")
) {
  void globalSetup();
}
