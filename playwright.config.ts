import { defineConfig, devices } from "@playwright/test";

export default defineConfig({
  testDir: "e2e/tests",
  globalSetup: "./e2e/scripts/fetch-fixtures.ts",
  // Real OCR + LLM are slow.
  timeout: 600_000,
  expect: { timeout: 30_000 },
  fullyParallel: false,
  workers: 1,
  reporter: process.env.CI ? "github" : "list",
  use: {
    baseURL: "http://localhost:1420",
    trace: "retain-on-failure",
    actionTimeout: 30_000,
    navigationTimeout: 30_000,
  },
  projects: [
    {
      name: "chromium",
      use: { ...devices["Desktop Chrome"] },
    },
  ],
  webServer: [
    {
      // Run the pre-built release binary directly so PATH-less shells
      // (pnpm webServer spawn) don't trip on `cargo`. Build it first via
      // `pnpm test-server:build`.
      command: "src-tauri/target/release/test-server",
      port: 1421,
      reuseExistingServer: !process.env.CI,
      timeout: 600_000,
      stdout: "pipe",
      stderr: "pipe",
    },
    {
      command: "pnpm dev",
      port: 1420,
      reuseExistingServer: !process.env.CI,
      timeout: 120_000,
      stdout: "pipe",
      stderr: "pipe",
    },
  ],
});
