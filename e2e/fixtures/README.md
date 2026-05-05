# E2E fixtures

`catan-zh-page.jpg` is copied from `src-tauri/tests/fixtures/page1.jpg` by
`e2e/scripts/fetch-fixtures.ts` (Playwright globalSetup). It is the same
synthetic Catan-style rules page that the cargo integration tests in
`src-tauri/tests/e2e_pipeline.rs` already exercise.

License: synthetic content authored for this project; redistributable.

We deliberately do not download from a public CDN at test time — that would
make the suite flaky and dependent on third-party uptime / licensing.
