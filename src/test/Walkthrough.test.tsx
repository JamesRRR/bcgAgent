import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, waitFor, act } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { Wrapper, makeAppCtx } from "./helpers";

// Mock the IPC surface — provide a session with one agent turn already, so
// the chat UI shows immediately and we can drive the volume button.
type Callback = (...args: unknown[]) => void;
const ttsDoneSubs: Callback[] = [];

vi.mock("@/lib/ipc", () => {
  return {
    games: {
      get: vi.fn(() =>
        Promise.resolve({
          id: "g1",
          name_zh: "测试桌游",
          name_en: null,
          publisher: null,
          cover_path: null,
          page_count: 1,
          created_at: 0,
        }),
      ),
    },
    walkthrough: {
      run: vi.fn(),
      getCached: vi.fn(() => Promise.resolve(null)),
      onToken: vi.fn(() => Promise.resolve(() => {})),
      onDone: vi.fn(() => Promise.resolve(() => {})),
    },
    walkthroughSession: {
      start: vi.fn(),
      continue_: vi.fn(),
      get: vi.fn(() =>
        Promise.resolve({
          session: {
            session_id: "s1",
            game_id: "g1",
            phase: "setup",
            created_at: 0,
            updated_at: 0,
          },
          turns: [
            {
              turn_no: 0,
              role: "agent",
              kind: "instruction",
              content:
                "<<PHASE:setup>>\n<<INSTRUCTION>>\n请把卡牌洗匀。\n<<END>>",
              created_at: 0,
            },
          ],
        }),
      ),
      reset: vi.fn(),
      onToken: vi.fn(() => Promise.resolve(() => {})),
      onDone: vi.fn(() => Promise.resolve(() => {})),
    },
    audio: {
      speak: vi.fn(() => Promise.resolve("h-1")),
      speakCancel: vi.fn(() => Promise.resolve()),
      onTtsDone: vi.fn((cb: Callback) => {
        ttsDoneSubs.push(cb);
        return Promise.resolve(() => {
          const i = ttsDoneSubs.indexOf(cb);
          if (i >= 0) ttsDoneSubs.splice(i, 1);
        });
      }),
      transcribeStreamStart: vi.fn(() => Promise.resolve()),
      transcribeChunk: vi.fn(() => Promise.resolve()),
      transcribeFinalize: vi.fn(() => Promise.resolve("")),
      transcribeStreamCancel: vi.fn(() => Promise.resolve()),
      micCaptureStart: vi.fn(() => Promise.resolve()),
      micCaptureStop: vi.fn(() => Promise.resolve("")),
      micCaptureCancel: vi.fn(() => Promise.resolve()),
      onTranscribePartial: vi.fn(() => Promise.resolve(() => {})),
    },
  };
});

import Walkthrough from "@/pages/Walkthrough";
import { audio } from "@/lib/ipc";

beforeEach(() => {
  vi.clearAllMocks();
  ttsDoneSubs.length = 0;
});

describe("Walkthrough — TTS stop button", () => {
  it("second click cancels even if backend `speak` is still in flight", async () => {
    const user = userEvent.setup();

    render(
      <Wrapper ctx={makeAppCtx({ selectedGameId: "g1" })}>
        <Walkthrough />
      </Wrapper>,
    );

    // Wait for the chat bubble to render so volume button is in the header.
    await screen.findByText(/请把卡牌洗匀/);

    const volBtnInitial = await screen.findByRole("button", {
      name: /开启语音朗读|Read aloud/i,
    });

    // First click: turn voice ON. The new flow doesn't auto-narrate when
    // toggling on — voice only narrates new agent bubbles. So we don't
    // expect speak to be called here. Confirm icon flipped instead.
    await user.click(volBtnInitial);

    await screen.findByRole("button", {
      name: /关闭语音朗读|Stop reading/i,
    });
    expect(audio.speak).not.toHaveBeenCalled();
  });

  it("clears speaking state when backend emits tts:done naturally", async () => {
    render(
      <Wrapper ctx={makeAppCtx({ selectedGameId: "g1" })}>
        <Walkthrough />
      </Wrapper>,
    );

    // Wait for the listener to be wired up.
    await waitFor(() => expect(ttsDoneSubs.length).toBeGreaterThan(0));

    // Simulate a tts:done event for an unknown handle — listener must be
    // resilient and not throw.
    await act(async () => {
      ttsDoneSubs[0]({ handle_id: "stranger" });
    });
  });
});
