import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { Wrapper, makeAppCtx } from "./helpers";

// IPC mock — must come before importing the page.
vi.mock("@/lib/ipc", () => {
  return {
    games: {
      list: vi.fn(),
      create: vi.fn(),
      get: vi.fn(),
      setCover: vi.fn(),
    },
    pages: { listByGame: vi.fn(), get: vi.fn() },
    search: { keyword: vi.fn(), semantic: vi.fn() },
    ingest: {
      run: vi.fn(),
      onPageStarted: vi.fn(() => Promise.resolve(() => {})),
      onPageDone: vi.fn(() => Promise.resolve(() => {})),
      onPageFailed: vi.fn(() => Promise.resolve(() => {})),
      onDone: vi.fn(() => Promise.resolve(() => {})),
    },
    ask: {
      run: vi.fn(),
      onCitations: vi.fn(() => Promise.resolve(() => {})),
      onToken: vi.fn(() => Promise.resolve(() => {})),
      onDone: vi.fn(() => Promise.resolve(() => {})),
    },
    audio: { transcribe: vi.fn(), speak: vi.fn(), speakCancel: vi.fn() },
    settings: {
      getSecret: vi.fn(),
      setSecret: vi.fn(),
      get: vi.fn(),
      set: vi.fn(),
    },
    qa: { list: vi.fn() },
  };
});

import Library from "@/pages/Library";
import { games as gamesIpc } from "@/lib/ipc";

beforeEach(() => {
  vi.clearAllMocks();
});

describe("Library", () => {
  it("shows empty state when no games exist", async () => {
    vi.mocked(gamesIpc.list).mockResolvedValueOnce([]);

    render(
      <Wrapper>
        <Library />
      </Wrapper>,
    );

    // i18n empty key
    expect(
      await screen.findByText(/书架空空如也|empty/i),
    ).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: /添加桌游|add game/i }),
    ).toBeInTheDocument();
  });

  it("opens add-game dialog and creates a game, then navigates to import", async () => {
    const user = userEvent.setup();
    const setPage = vi.fn();
    const ctx = makeAppCtx({ setPage });

    vi.mocked(gamesIpc.list).mockResolvedValueOnce([]);
    vi.mocked(gamesIpc.create).mockResolvedValueOnce("game-uuid-123");

    render(
      <Wrapper ctx={ctx}>
        <Library />
      </Wrapper>,
    );

    // Click Add Game button
    const addBtn = await screen.findByRole("button", {
      name: /添加桌游|add game/i,
    });
    await user.click(addBtn);

    // Dialog opens — find the Chinese-name input
    const dialog = await screen.findByRole("dialog");
    expect(dialog).toBeInTheDocument();

    // The first text input is the Chinese name
    const inputs = dialog.querySelectorAll('input[type="text"]');
    expect(inputs.length).toBe(3);

    await user.type(inputs[0] as HTMLInputElement, "卡坦岛");
    await user.type(inputs[1] as HTMLInputElement, "Catan");
    await user.type(inputs[2] as HTMLInputElement, "Kosmos");

    // Submit — confirm button (the second / submit button)
    const confirmBtn = screen.getByRole("button", { name: /创建|confirm/i });
    await user.click(confirmBtn);

    await waitFor(() => {
      expect(gamesIpc.create).toHaveBeenCalledWith(
        "卡坦岛",
        "Catan",
        "Kosmos",
      );
    });
    await waitFor(() => {
      expect(setPage).toHaveBeenCalledWith("import", "game-uuid-123");
    });
  });

  it("submit button is disabled when name_zh is empty", async () => {
    const user = userEvent.setup();
    vi.mocked(gamesIpc.list).mockResolvedValueOnce([]);

    render(
      <Wrapper>
        <Library />
      </Wrapper>,
    );

    await user.click(
      await screen.findByRole("button", { name: /添加桌游|add game/i }),
    );

    const confirmBtn = await screen.findByRole("button", {
      name: /创建|confirm/i,
    });
    expect(confirmBtn).toBeDisabled();

    // After typing, it enables
    const dialog = screen.getByRole("dialog");
    const zhInput = dialog.querySelectorAll('input[type="text"]')[0] as HTMLInputElement;
    await user.type(zhInput, "X");
    expect(confirmBtn).not.toBeDisabled();
  });

  it("renders populated shelf with game cards and navigates on click", async () => {
    const user = userEvent.setup();
    const setPage = vi.fn();
    const ctx = makeAppCtx({ setPage });

    vi.mocked(gamesIpc.list).mockResolvedValueOnce([
      {
        id: "g1",
        name_zh: "卡坦岛",
        name_en: "Catan",
        publisher: null,
        cover_path: null,
        page_count: 12,
        created_at: 1730000000,
      },
      {
        id: "g2",
        name_zh: "翼展",
        name_en: "Wingspan",
        publisher: null,
        cover_path: null,
        page_count: 28,
        created_at: 1730000010,
      },
    ]);

    render(
      <Wrapper ctx={ctx}>
        <Library />
      </Wrapper>,
    );

    expect(await screen.findByText("卡坦岛")).toBeInTheDocument();
    expect(screen.getByText("翼展")).toBeInTheDocument();

    // Click the first card
    await user.click(screen.getByText("卡坦岛"));
    expect(setPage).toHaveBeenCalledWith("handbook", "g1");
  });
});
