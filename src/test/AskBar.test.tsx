import { describe, it, expect, vi } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { Wrapper } from "./helpers";

// VoiceButton uses ipc.audio under the hood; stub the entire module so the
// AskBar tests stay focused on the search-web button.
vi.mock("@/lib/ipc", () => ({
  audio: {
    micCaptureStart: vi.fn(() => Promise.resolve()),
    micCaptureStop: vi.fn(() => Promise.resolve("")),
    micCaptureCancel: vi.fn(() => Promise.resolve()),
    onTranscribePartial: vi.fn(() => Promise.resolve(() => {})),
  },
}));

import AskBar from "@/components/ask/AskBar";

describe("AskBar — 🔍 search-web button", () => {
  it("toggles the force-research flag, then resets after submit", async () => {
    const user = userEvent.setup();
    const onSubmit = vi.fn();
    render(
      <Wrapper>
        <AskBar busy={false} value="" onChange={() => {}} onSubmit={onSubmit} />
      </Wrapper>,
    );
    const btn = screen.getByTestId("ask-search-web-btn");
    expect(btn).toHaveAttribute("aria-pressed", "false");
    await user.click(btn);
    expect(btn).toHaveAttribute("aria-pressed", "true");
    await user.click(btn);
    expect(btn).toHaveAttribute("aria-pressed", "false");
  });

  it("passes forceResearch=true when toggled and Send is clicked", async () => {
    const user = userEvent.setup();
    const onSubmit = vi.fn();
    let value = "强盗?";
    const onChange = (v: string) => {
      value = v;
    };
    const { rerender } = render(
      <Wrapper>
        <AskBar busy={false} value={value} onChange={onChange} onSubmit={onSubmit} />
      </Wrapper>,
    );
    rerender(
      <Wrapper>
        <AskBar busy={false} value={value} onChange={onChange} onSubmit={onSubmit} />
      </Wrapper>,
    );
    await user.click(screen.getByTestId("ask-search-web-btn"));
    await user.click(screen.getByRole("button", { name: /发送|send/i }));
    expect(onSubmit).toHaveBeenCalledWith("强盗?", true);
  });

  it("forceResearch defaults to false", async () => {
    const user = userEvent.setup();
    const onSubmit = vi.fn();
    render(
      <Wrapper>
        <AskBar busy={false} value="hi" onChange={() => {}} onSubmit={onSubmit} />
      </Wrapper>,
    );
    await user.click(screen.getByRole("button", { name: /发送|send/i }));
    expect(onSubmit).toHaveBeenCalledWith("hi", false);
  });
});
