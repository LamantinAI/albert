import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";

import { Markdown } from "./Markdown";

describe("Markdown", () => {
  it("renders readable code and tables without executing raw HTML or remote image requests", () => {
    const { container } = render(
      <Markdown
        text={
          "Hello **Albert**\n\n<script>alert(1)</script>\n\n![tracking](https://external.invalid/pixel.png)\n\n[unsafe](javascript:alert(1))\n\n```rust\nlet answer = 42;\n```\n\n| Task | State |\n| --- | --- |\n| Read | Done |"
        }
      />,
    );
    expect(screen.getByText("Albert")).toBeVisible();
    expect(screen.getByText("let answer = 42;")).toBeVisible();
    expect(screen.getByRole("table")).toBeVisible();
    expect(container.querySelector("script")).toBeNull();
    expect(container.querySelector("img")).toBeNull();
    expect(screen.getByText("unsafe")).not.toHaveAttribute(
      "href",
      expect.stringContaining("javascript:"),
    );
  });
});
