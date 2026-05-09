import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { Wrapper } from "./helpers";

// IPC mocks for the Ask flow.
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
    },
    audio: {
      transcribe: vi.fn(),
      speak: vi.fn(() => Promise.resolve("tts-handle")),
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
import { ask as askIpc, audio } from "@/lib/ipc";

beforeEach(() => {
  vi.clearAllMocks();
  subs.citations = [];
  subs.token = [];
  subs.done = [];
});

describe("Voice flow", () => {
  it("hold-to-record → release → transcribe → input fills → auto-submit ask", async () => {
    const user = userEvent.setup();

    vi.mocked(audio.micCaptureStop).mockResolvedValueOnce("强盗怎么移动？");

    render(
      <Wrapper>
        <Ask />
      </Wrapper>,
    );

    const micBtn = await screen.findByRole("button", { name: /record/i });

    // pointerDown → starts native cpal capture
    await user.pointer({ keys: "[MouseLeft>]", target: micBtn });
    await waitFor(() => {
      expect(audio.micCaptureStart).toHaveBeenCalled();
    });

    // pointerUp → stops capture, returns transcript, auto-submits
    await user.pointer({ keys: "[/MouseLeft]" });
    await waitFor(() => {
      expect(audio.micCaptureStop).toHaveBeenCalled();
    });
    await waitFor(() => {
      expect(askIpc.run).toHaveBeenCalledWith("强盗怎么移动？", null);
    });
  });

  it("toaster surfaces backend mic-capture errors", async () => {
    const user = userEvent.setup();

    vi.mocked(audio.micCaptureStart).mockRejectedValueOnce(
      "audio: no default input device",
    );

    render(
      <Wrapper>
        <Ask />
      </Wrapper>,
    );

    const micBtn = await screen.findByRole("button", { name: /record/i });
    await user.pointer({ keys: "[MouseLeft>]", target: micBtn });

    expect(
      await screen.findByText(/no default input device/i),
    ).toBeInTheDocument();
  });
});
