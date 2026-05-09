import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { Wrapper } from "./helpers";

vi.mock("@/lib/ipc", () => ({
  research: {
    endorseChunk: vi.fn(() => Promise.resolve()),
  },
}));

import CitationChip from "@/components/ask/CitationChip";
import { research as researchIpc, type RetrievedChunk } from "@/lib/ipc";

const baseChunk: RetrievedChunk = {
  chunk_id: 7,
  game_id: "g1",
  game_name: "卡坦岛",
  page_id: "p1",
  page_number: 5,
  heading_path: null,
  content: "...",
  fused_score: 0.8,
};

beforeEach(() => {
  vi.clearAllMocks();
});

describe("CitationChip — trust badges", () => {
  it("renders the publisher badge by default (BookOpen)", () => {
    render(
      <Wrapper>
        <CitationChip
          chunk={{ ...baseChunk, trust_tier: "publisher" }}
          onOpen={() => {}}
        />
      </Wrapper>,
    );
    expect(screen.getByTestId("citation-chip-publisher")).toBeInTheDocument();
    expect(screen.getByLabelText("publisher")).toBeInTheDocument();
  });

  it("renders the designer badge", () => {
    render(
      <Wrapper>
        <CitationChip
          chunk={{ ...baseChunk, trust_tier: "designer" }}
          onOpen={() => {}}
        />
      </Wrapper>,
    );
    expect(screen.getByTestId("citation-chip-designer")).toBeInTheDocument();
    expect(screen.getByLabelText("designer")).toBeInTheDocument();
  });

  it("renders the community badge", () => {
    render(
      <Wrapper>
        <CitationChip
          chunk={{ ...baseChunk, trust_tier: "community" }}
          onOpen={() => {}}
        />
      </Wrapper>,
    );
    expect(screen.getByTestId("citation-chip-community")).toBeInTheDocument();
    expect(screen.getByLabelText("community")).toBeInTheDocument();
  });

  it("renders the unverified badge", () => {
    render(
      <Wrapper>
        <CitationChip
          chunk={{ ...baseChunk, trust_tier: "unverified" }}
          onOpen={() => {}}
        />
      </Wrapper>,
    );
    expect(screen.getByTestId("citation-chip-unverified")).toBeInTheDocument();
    expect(screen.getByLabelText("unverified")).toBeInTheDocument();
  });

  it("falls back to publisher when no tier present (legacy payload)", () => {
    render(
      <Wrapper>
        <CitationChip chunk={{ ...baseChunk }} onOpen={() => {}} />
      </Wrapper>,
    );
    expect(screen.getByTestId("citation-chip-publisher")).toBeInTheDocument();
  });

  it("clicking community chip with source_url opens that URL", async () => {
    const user = userEvent.setup();
    const onOpen = vi.fn();
    const openSpy = vi.spyOn(window, "open").mockImplementation(() => null);
    render(
      <Wrapper>
        <CitationChip
          chunk={{
            ...baseChunk,
            trust_tier: "community",
            source_url: "https://bgg/x",
          }}
          onOpen={onOpen}
        />
      </Wrapper>,
    );
    await user.click(screen.getByTestId("citation-chip-community"));
    expect(openSpy).toHaveBeenCalledWith(
      "https://bgg/x",
      "_blank",
      "noopener,noreferrer",
    );
    expect(onOpen).not.toHaveBeenCalled();
    openSpy.mockRestore();
  });

  it("clicking publisher chip falls through to onOpen", async () => {
    const user = userEvent.setup();
    const onOpen = vi.fn();
    render(
      <Wrapper>
        <CitationChip
          chunk={{ ...baseChunk, trust_tier: "publisher" }}
          onOpen={onOpen}
        />
      </Wrapper>,
    );
    await user.click(screen.getByTestId("citation-chip-publisher"));
    expect(onOpen).toHaveBeenCalledWith("g1");
  });

  it("thumbs-up calls research.endorseChunk", async () => {
    const user = userEvent.setup();
    render(
      <Wrapper>
        <CitationChip
          chunk={{ ...baseChunk, trust_tier: "community" }}
          onOpen={() => {}}
        />
      </Wrapper>,
    );
    await user.click(screen.getByTestId("citation-thumbs-up"));
    expect(researchIpc.endorseChunk).toHaveBeenCalledWith(7, true);
  });
});
