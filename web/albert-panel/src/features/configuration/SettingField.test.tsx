import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { expect, test, vi } from "vitest";
import { SettingField } from "./SettingField";
import type { ConfigField } from "./useStructuredDraft";
const numeric: ConfigField = {
  path: "reflection.period_secs",
  group: "Reflection",
  label: "Reflection interval (seconds)",
  kind: "integer",
  value: null,
  default_value: 21600,
  description: "Reflection schedule.",
  minimum: 1,
  maximum: 9007199254740991,
};
test("effective defaults are visible and changing a display unit does not create an override", async () => {
  const change = vi.fn();
  render(<SettingField field={numeric} disabled={false} onChange={change} />);
  expect(
    screen.getByRole("spinbutton", { name: "Reflection interval" }),
  ).toHaveValue(6);
  await userEvent.click(
    screen.getByRole("combobox", {
      name: "Unit for Reflection interval (seconds)",
    }),
  );
  await userEvent.click(screen.getByRole("option", { name: "Minutes" }));
  expect(
    screen.getByRole("spinbutton", { name: "Reflection interval" }),
  ).toHaveValue(360);
  expect(change).not.toHaveBeenCalled();
});
test("explicit false and zero remain visible, and reset removes an override", async () => {
  const change = vi.fn();
  const view = render(
    <SettingField
      field={{
        ...numeric,
        path: "agent.max_tool_turns",
        label: "Tool rounds",
        value: 0,
        default_value: 8,
        minimum: 0,
      }}
      disabled={false}
      onChange={change}
    />,
  );
  expect(screen.getByRole("spinbutton", { name: "Tool rounds" })).toHaveValue(
    0,
  );
  await userEvent.click(
    screen.getByRole("button", { name: "Use default for Tool rounds" }),
  );
  expect(change).toHaveBeenLastCalledWith("agent.max_tool_turns", "");
  view.rerender(
    <SettingField
      field={{
        ...numeric,
        path: "stream_status",
        label: "Live progress",
        kind: "boolean",
        value: false,
        default_value: true,
      }}
      disabled={false}
      onChange={change}
    />,
  );
  expect(screen.getByRole("switch", { name: "Live progress" })).toHaveAttribute(
    "aria-checked",
    "false",
  );
  await userEvent.click(
    screen.getByRole("button", { name: "Use default for Live progress" }),
  );
  expect(change).toHaveBeenLastCalledWith("stream_status", "default");
});

test("a retained invalid numeric draft restores its text and unit after remount", () => {
  const change = vi.fn();
  render(
    <SettingField
      field={numeric}
      edit="invalid:minutes:1.333"
      disabled={false}
      onChange={change}
    />,
  );
  expect(
    screen.getByRole("spinbutton", { name: "Reflection interval" }),
  ).toHaveValue(1.333);
  expect(
    screen.getByRole("spinbutton", { name: "Reflection interval" }),
  ).toHaveAttribute("aria-invalid", "true");
  expect(
    screen.getByRole("combobox", {
      name: "Unit for Reflection interval (seconds)",
    }),
  ).toHaveTextContent("Minutes");
  expect(change).not.toHaveBeenCalled();
});
