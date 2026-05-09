import { describe, it, expect, vi } from "vitest";
import { render } from "@testing-library/react";

vi.mock("@tauri-apps/api/core", () => ({
  convertFileSrc: (p: string) => `tauri://${p}`,
}));

vi.mock("@/lib/transport", () => ({
  inTauri: true,
  invoke: vi.fn(),
  listen: vi.fn(),
}));

import MarkdownView from "@/components/handbook/MarkdownView";

describe("MarkdownView — illustration anchors", () => {
  it("renders ill: src as a real <figure> when illustrations map provides it", () => {
    const md = "Here is a card anatomy: ![羽毛栏](ill:0)\n\nMore text.";
    const { container } = render(
      <MarkdownView
        source={md}
        illustrations={{
          "ill:0": {
            image_path: "/games/g1/illustrations/p1_0.jpg",
            label: "羽毛栏",
          },
        }}
      />,
    );
    const img = container.querySelector("img");
    expect(img).toBeTruthy();
    expect(img?.getAttribute("src")).toContain(
      "tauri:///games/g1/illustrations/p1_0.jpg",
    );
    const fig = container.querySelector("[role='figure']");
    expect(fig?.textContent).toContain("羽毛栏");
  });

  it("falls back to placeholder when token has no entry", () => {
    const md = "Here is: ![label](ill:9)";
    const { container } = render(
      <MarkdownView source={md} illustrations={{}} />,
    );
    expect(container.querySelector("[role='figure']")).toBeFalsy();
  });
});
