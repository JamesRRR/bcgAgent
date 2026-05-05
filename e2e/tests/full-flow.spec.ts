import { test, expect } from "@playwright/test";
import { resolve, dirname } from "node:path";
import { fileURLToPath } from "node:url";

const HERE = dirname(fileURLToPath(import.meta.url));
const FIXTURE = resolve(HERE, "..", "fixtures", "catan-zh-page.jpg");

const RESET_URL = "http://localhost:1421/api/__test/reset";

test.beforeEach(async ({ request }) => {
  // Wipe DB rows + per-game asset dirs (preserves models/secrets).
  const res = await request.post(RESET_URL);
  expect(res.ok()).toBeTruthy();
});

test("full click-driven flow: add game, ingest page, ask, verify streaming", async ({
  page,
}) => {
  page.on("pageerror", (err) => console.log("PAGEERROR:", err.message));
  // 1. Open the app. The Library lands either in empty-state (after a reset)
  //    or populated; either way the "添加桌游" button is present.
  await page.goto("/");
  await expect(page.getByRole("button", { name: /添加桌游/ })).toBeVisible();

  // 2. Open the add-game dialog.
  await page.getByRole("button", { name: /添加桌游/ }).click();
  await expect(page.getByRole("dialog")).toBeVisible();

  // 3. Fill in the new-game form.
  await page.getByTestId("new-game-name-zh").fill("卡坦岛");
  await page.getByTestId("new-game-name-en").fill("Catan");
  await page.getByTestId("new-game-publisher").fill("Kosmos");

  // 4. Submit -> navigates to Import page.
  await page.getByRole("button", { name: /创建/ }).click();
  await expect(page.getByRole("heading", { name: "导入规则书" })).toBeVisible({
    timeout: 30_000,
  });
  // The dropzone is the user-facing entrypoint; the hidden picker mounts on
  // the Dropzone's first effect.
  await expect(page.getByTestId("dropzone")).toBeVisible();

  // 5. Inject the fixture image into the hidden file picker. The Dropzone's
  //    global PICKER_READY_EVENT listener uploads the file and forwards the
  //    server-returned absolute path to the page-card list.
  const fileInput = page.getByTestId("bcg-hidden-file-picker");
  await expect(fileInput).toBeAttached({ timeout: 10_000 });
  await fileInput.setInputFiles(FIXTURE);

  // 6. Page card appears with status `pending`.
  const card = page.getByTestId("page-card").first();
  await expect(card).toBeVisible({ timeout: 30_000 });

  // 7. Start the ingest.
  await page.getByRole("button", { name: /开始导入/ }).click();

  // 8. Watch progress: pending -> running. OCR + embeddings are real; the
  //    bge-m3 model may take a moment to load, plus a Qwen-VL round trip.
  await expect(card).toHaveAttribute("data-status", /running|done/, {
    timeout: 30_000,
  });

  // 9. The `ingest:done` event navigates to the Handbook page. We wait on the
  //    navigation rather than the intermediate "done" status — React may
  //    batch the final state update with the navigation, so the card can
  //    unmount before its `data-status="done"` is observable.
  await expect(page.getByPlaceholder("搜索规则书…")).toBeVisible({
    timeout: 180_000,
  });

  // 10. Search inside the handbook for "强盗" (Catan robber). The fixture text
  //     contains this term, and FTS5 + jieba should hit it.
  const searchBox = page.getByPlaceholder("搜索规则书…");
  await searchBox.fill("强盗");
  // Search results in the sidebar (TocSidebar shows hits when query non-empty).
  // We just assert that *something* mentioning the term shows up — either a
  // highlighted snippet in the reader or a hit row in the sidebar.
  await expect(
    page.locator("text=强盗").first(),
  ).toBeVisible({ timeout: 30_000 });

  // 11. Switch to the Ask page via sidebar nav.
  await page.getByRole("button", { name: "问规则" }).click();
  await expect(page.getByRole("heading", { name: "问规则" })).toBeVisible();

  // 12. Type the question and submit.
  const askInput = page.getByTestId("ask-input");
  await askInput.fill("玩家掷出 7 点之后强盗怎么处理？");
  await page.getByRole("button", { name: /发送/ }).click();

  // 13. Wait for streamed tokens to appear in the answer card.
  const answer = page.getByTestId("answer-text");
  await expect(answer).toBeVisible();
  await expect(async () => {
    const txt = (await answer.textContent()) ?? "";
    expect(txt.length).toBeGreaterThan(8);
  }).toPass({ timeout: 180_000 });

  // 14. Citations chip group renders.
  await expect(page.getByTestId("citations")).toBeVisible({ timeout: 60_000 });

  // 15. Answer mentions a grounding term from the source page (broad set so
  //     we don't over-fit phrasing).
  await expect(async () => {
    const txt = ((await answer.textContent()) ?? "").toLowerCase();
    const grounded = ["强盗", "robber", "沙漠", "资源", "7", "七"].some((kw) =>
      txt.includes(kw.toLowerCase()),
    );
    expect(grounded, `answer didn't reference source terms: ${txt}`).toBe(true);
  }).toPass({ timeout: 30_000 });

  // 16. The question shows up in the history sidebar.
  await expect(
    page.locator("text=玩家掷出 7 点之后强盗怎么处理？").first(),
  ).toBeVisible({ timeout: 15_000 });
});
