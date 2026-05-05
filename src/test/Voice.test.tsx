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

// Stub the WAV encoder so we don't need a real AudioContext in jsdom.
vi.mock("@/components/ask/wav", () => ({
  blobToWav16k: vi.fn(async () => new Uint8Array([82, 73, 70, 70])), // "RIFF"
}));

import Ask from "@/pages/Ask";
import { ask as askIpc, audio } from "@/lib/ipc";

// MediaRecorder + getUserMedia stubs
class FakeMediaRecorder {
  state: "inactive" | "recording" | "paused" = "inactive";
  ondataavailable: ((e: { data: Blob }) => void) | null = null;
  onstop: (() => void) | null = null;
  mimeType = "audio/webm";

  constructor(_stream: MediaStream) {}

  start() {
    this.state = "recording";
  }
  stop() {
    this.state = "inactive";
    this.ondataavailable?.({ data: new Blob(["fake-audio"]) });
    this.onstop?.();
  }
}

beforeEach(() => {
  vi.clearAllMocks();
  subs.citations = [];
  subs.token = [];
  subs.done = [];

  // @ts-expect-error stub
  global.MediaRecorder = FakeMediaRecorder;

  Object.defineProperty(navigator, "mediaDevices", {
    configurable: true,
    value: {
      getUserMedia: vi.fn(async () => ({
        getTracks: () => [{ stop: vi.fn() }],
      })),
    },
  });
});

describe("Voice flow", () => {
  it("hold-to-record → release → transcribe → input fills → auto-submit ask", async () => {
    const user = userEvent.setup();

    vi.mocked(audio.transcribe).mockResolvedValueOnce("强盗怎么移动？");

    render(
      <Wrapper>
        <Ask />
      </Wrapper>,
    );

    const micBtn = await screen.findByRole("button", { name: /record/i });

    // pointerDown → starts MediaRecorder
    await user.pointer({ keys: "[MouseLeft>]", target: micBtn });
    await waitFor(() =>
      expect(navigator.mediaDevices.getUserMedia).toHaveBeenCalled(),
    );

    // pointerUp → stops recorder, triggers transcribe + auto-submit
    await user.pointer({ keys: "[/MouseLeft]" });

    await waitFor(() => {
      expect(audio.transcribe).toHaveBeenCalled();
    });

    await waitFor(() => {
      expect(askIpc.run).toHaveBeenCalledWith("强盗怎么移动？", null);
    });
  });

  it("toaster surfaces denied microphone permission", async () => {
    const user = userEvent.setup();

    Object.defineProperty(navigator, "mediaDevices", {
      configurable: true,
      value: {
        getUserMedia: vi.fn(async () => {
          throw new DOMException("denied", "NotAllowedError");
        }),
      },
    });

    render(
      <Wrapper>
        <Ask />
      </Wrapper>,
    );

    const micBtn = await screen.findByRole("button", { name: /record/i });
    await user.pointer({ keys: "[MouseLeft>]", target: micBtn });
    await user.pointer({ keys: "[/MouseLeft]" });

    expect(
      await screen.findByText(/麦克风权限被拒绝|Microphone permission denied/i),
    ).toBeInTheDocument();
  });
});
