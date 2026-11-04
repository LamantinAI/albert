import { render } from "@testing-library/react";
import { beforeEach, afterEach, expect, test, vi } from "vitest";
import { Markdown } from "./Markdown";

beforeEach(() =>
  vi.stubGlobal("matchMedia", () => ({
    matches: true,
    addEventListener() {},
    removeEventListener() {},
  })),
);
afterEach(() => vi.unstubAllGlobals());
test("block cursor and fresh token spans preserve formatted and escaped text", () => {
  const view = render(<Markdown streaming text="Hello **world**" />);
  expect(view.container.querySelector("strong")).toHaveTextContent("world");
  expect(view.container.querySelectorAll(".stream-cursor")).toHaveLength(1);
  expect(view.container.querySelector(".stream-edge")).toHaveTextContent(
    "Hello",
  );
  expect(view.container.querySelector(".stream-cursor")).toHaveTextContent("");
  expect(
    [...view.container.querySelectorAll(".stream-edge")]
      .map((node) => node.textContent)
      .join(""),
  ).toBe("Hello world");
  view.rerender(<Markdown streaming text={"```rust\nlet a = 1;\n```"} />);
  expect(view.container.querySelector("pre code")).toHaveTextContent(
    "let a = 1;",
  );
  expect(view.container.querySelectorAll(".stream-cursor")).toHaveLength(1);
  view.rerender(
    <Markdown streaming text={"Hello\n<script>window.bad=1</script>"} />,
  );
  expect(view.container.querySelector("script")).toBeNull();
});

test("live text remains readable with only a block cursor and subtle ink", () => {
  const text = "Received readable text";
  const view = render(
    <Markdown
      streaming
      text={text}
      ink={{ ranges: [{ from: 0, to: text.length, born: 0 }], now: 24 }}
    />,
  );
  expect(view.container.textContent).toBe(text);
  expect(view.container.querySelector(".stream-code")).toBeNull();
  expect(view.container.querySelectorAll(".stream-cursor")).toHaveLength(1);
  expect(view.container.querySelector(".stream-cursor")!.textContent).toBe("");
  view.rerender(<Markdown text={text} />);
  expect(view.container.textContent).toBe(text);
  expect(view.container.querySelector(".stream-cursor")).toBeNull();
});
