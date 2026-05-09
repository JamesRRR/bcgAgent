import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, waitFor, act } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { Wrapper } from "./helpers";

// Programmable event subscribers that tests can fire.
type Callback = (...args: unknown[]) => void;
const subs: Record<string, Callback[]> = {
  citations: [],
  token: [],
  done: [],
};

function subscribe(name: keyof typeof subs) {
  return (cb: Callback) => {
    subs[name].push(cb);
    return Promise.resolve(() => {
      subs[name] = subs[name].filter((c) => c !== cb);
    });
  };
}

vi.mock("@/lib/ipc", () => {
  return {
    games: {
      list: vi.fn(() => Promise.resolve([])),
      create: vi.fn(),
      get: vi.fn(),
      setCover: vi.fn(),
    },
    pages: { listByGame: vi.fn(() => Promise.resolve([])), get: vi.fn() },
    search: { keyword: vi.fn(), semantic: vi.fn() },
    ingest: {
      run: vi.fn(),
      onPageStarted: vi.fn(() => Promise.resolve(() => {})),
      onPageDone: vi.fn(() => Promise.resolve(() => {})),
      onPageFailed: vi.fn(() => Promise.resolve(() => {})),
      onDone: vi.fn(() => Promise.resolve(() => {})),
    },
    ask: {
      run: vi.fn(() => Promise.resolve("qa-1")),
      onCitations: vi.fn((cb: Callback) => subscribe("citations")(cb)),
      onToken: vi.fn((cb: Callback) => subscribe("token")(cb)),
      onDone: vi.fn((cb: Callback) => subscribe("done")(cb)),
      onResearchStarted: vi.fn(() => Promise.resolve(() => {})),
      onResearchDone: vi.fn(() => Promise.resolve(() => {})),
    },
    research: {
      explicit: vi.fn(() =>
        Promise.resolve({
          event_id: 1,
          chunks_added: 0,
          urls_fetched: [],
          timed_out: false,
        }),
      ),
      endorseChunk: vi.fn(() => Promise.resolve()),
      runExtractors: vi.fn(),
      kbDiff: vi.fn(),
      onSeedCrawlDone: vi.fn(() => Promise.resolve(() => {})),
    },
    audio: {
      transcribe: vi.fn(),
      speak: vi.fn(() => Promise.resolve("tts-handle-1")),
      speakCancel: vi.fn(() => Promise.resolve()),
      micCaptureStart: vi.fn(() => Promise.resolve()),
      micCaptureStop: vi.fn(() => Promise.resolve("")),
      micCaptureCancel: vi.fn(() => Promise.resolve()),
      onTranscribePartial: vi.fn(() => Promise.resolve(() => {})),
    },
    settings: {
      getSecret: vi.fn(),
      setSecret: vi.fn(),
      get: vi.fn(),
      set: vi.fn(),
    },
    qa: { list: vi.fn(() => Promise.resolve([])) },
  };
});

import Ask from "@/pages/Ask";
import { ask as askIpc, audio, qa as qaIpc } from "@/lib/ipc";

beforeEach(() => {
  vi.clearAllMocks();
  subs.citations = [];
  subs.token = [];
  subs.done = [];
  vi.mocked(qaIpc.list).mockResolvedValue([]);
});

describe("Ask page — text flow", () => {
  it("submits a typed question via Send button and streams tokens", async () => {
    const user = userEvent.setup();

    render(
      <Wrapper>
        <Ask />
      </Wrapper>,
    );

    const input = await screen.findByPlaceholderText(
      /提问吧|ask|城堡怎么建/i,
    );
    await user.type(input, "强盗怎么移动？");

    const sendBtn = screen.getByRole("button", { name: /发送|send/i });
    await user.click(sendBtn);

    await waitFor(() => {
      expect(askIpc.run).toHaveBeenCalledWith("强盗怎么移动？", null);
    });

    // Simulate the backend streaming tokens via the registered listeners.
    expect(subs.token.length).toBe(1);
    expect(subs.citations.length).toBe(1);
    expect(subs.done.length).toBe(1);

    act(() => {
      subs.citations[0]([
        {
          chunk_id: 1,
          game_id: "g1",
          game_name: "卡坦岛",
          page_id: "p1",
          page_number: 5,
          heading_path: "强盗规则",
          content: "...",
          fused_score: 0.9,
        },
      ]);
    });

    act(() => {
      subs.token[0]("强盗");
      subs.token[0]("移到");
      subs.token[0]("沙漠。");
    });

    await waitFor(() => {
      expect(screen.getByText(/强盗移到沙漠。/)).toBeInTheDocument();
    });

    act(() => {
      subs.done[0]({ qa_id: "qa-1" });
    });

    // Citations panel renders the citation chip
    expect(screen.getByText(/卡坦岛/)).toBeInTheDocument();
  });

  it("Send button is disabled when input is empty", async () => {
    render(
      <Wrapper>
        <Ask />
      </Wrapper>,
    );
    const sendBtn = await screen.findByRole("button", { name: /发送|send/i });
    expect(sendBtn).toBeDisabled();
  });

  it("Enter key submits", async () => {
    const user = userEvent.setup();
    render(
      <Wrapper>
        <Ask />
      </Wrapper>,
    );
    const input = await screen.findByPlaceholderText(
      /提问吧|ask|城堡怎么建/i,
    );
    await user.type(input, "怎么建城?{Enter}");
    await waitFor(() => {
      expect(askIpc.run).toHaveBeenCalledWith("怎么建城?", null);
    });
  });

  it("auto-cancels in-flight TTS when starting a new ask", async () => {
    const user = userEvent.setup();

    render(
      <Wrapper>
        <Ask />
      </Wrapper>,
    );

    // Turn TTS on so the previous answer would speak
    const ttsBtn = screen.getByRole("button", {
      name: /朗读|playTTS|stopTTS|stop|play/i,
    });
    await user.click(ttsBtn);

    // Submit first question
    const input = await screen.findByPlaceholderText(
      /提问吧|ask|城堡怎么建/i,
    );
    await user.type(input, "Q1{Enter}");
    await waitFor(() => expect(askIpc.run).toHaveBeenCalledTimes(1));

    act(() => {
      subs.token[0]("answer1");
      subs.done[0]({ qa_id: "qa-1" });
    });
    await waitFor(() => expect(audio.speak).toHaveBeenCalledTimes(1));

    // Submit second question — should cancel the prior TTS handle
    await user.type(input, "Q2{Enter}");
    await waitFor(() => expect(askIpc.run).toHaveBeenCalledTimes(2));
    expect(audio.speakCancel).toHaveBeenCalledWith("tts-handle-1");
  });
});
