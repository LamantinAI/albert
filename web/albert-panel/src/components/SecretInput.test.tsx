import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { createRef } from "react";
import { expect, test, vi } from "vitest";

import { SecretInput } from "./SecretInput";
import { Field } from "./ui";

test("shared password fields reveal independently without submitting or changing their values", async () => {
  const user = userEvent.setup(),
    submit = vi.fn((event) => event.preventDefault()),
    change = vi.fn();
  render(
    <form onSubmit={submit}>
      <Field
        label="Password"
        type="password"
        value="fixture-password"
        onChange={change}
        hint="A private field"
      />
      <Field label="API key" type="password" defaultValue="fixture-key" />
    </form>,
  );
  const password = screen.getByLabelText("Password"),
    key = screen.getByLabelText("API key");
  expect(password).toHaveAttribute("type", "password");
  expect(password).toHaveAccessibleDescription("A private field");
  await user.click(password);
  await user.click(screen.getAllByRole("button", { name: "Show secret" })[0]);
  expect(password).toHaveAttribute("type", "text");
  expect(password).toHaveFocus();
  expect(password).toHaveValue("fixture-password");
  expect(key).toHaveAttribute("type", "password");
  expect(change).not.toHaveBeenCalled();
  expect(submit).not.toHaveBeenCalled();
  await user.tab();
  expect(screen.getByRole("button", { name: "Hide secret" })).toHaveFocus();
  await user.keyboard(" ");
  expect(password).toHaveAttribute("type", "password");
  await user.keyboard(" ");
  expect(password).toHaveAttribute("type", "text");
  await user.tab();
  expect(key).toHaveFocus();
  expect(password).toHaveAttribute("type", "password");
});

test("uncontrolled secret fields keep their original ref and DOM value through visibility changes", async () => {
  const user = userEvent.setup(),
    ref = createRef<HTMLInputElement>();
  const { rerender } = render(<SecretInput ref={ref} aria-label="Value" />);
  const input = ref.current!;
  await user.type(input, "fixture-secret");
  expect(input.getAttribute("value")).toBeNull();
  await user.click(screen.getByRole("button", { name: "Show secret" }));
  expect(ref.current).toBe(input);
  expect(input).toHaveAttribute("type", "text");
  expect(input).toHaveValue("fixture-secret");
  expect(input.getAttribute("value")).toBeNull();
  rerender(<SecretInput ref={ref} aria-label="Value" disabled />);
  expect(input).toHaveAttribute("type", "password");
  expect(screen.getByRole("button", { name: "Show secret" })).toBeDisabled();
  rerender(<SecretInput ref={ref} aria-label="Value" />);
  expect(input).toHaveAttribute("type", "password");
});
