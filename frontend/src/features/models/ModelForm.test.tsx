import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { expect, it, vi } from "vitest";

import { ModelForm } from "./ModelForm";

it("preserves a host-managed subscription endpoint when editing its model metadata", async () => {
  const save = vi.fn().mockResolvedValue(undefined);
  render(
    <ModelForm
      initial={{
        config: {
          id: "host-subscription",
          model: "host-model",
          provider: "subscription",
          tools: true,
          vision: true,
        },
        credential_configured: true,
        priority: 0,
      }}
      onSave={save}
    />,
  );
  expect(
    screen.getByLabelText("OpenAI-compatible API base URL"),
  ).toBeDisabled();
  expect(screen.getByLabelText("OpenAI-compatible API base URL")).toHaveValue(
    "",
  );
  expect(screen.queryByLabelText("API key")).toBeNull();
  fireEvent.click(screen.getByRole("button", { name: "Add model" }));
  await waitFor(() =>
    expect(save).toHaveBeenCalledWith(
      expect.objectContaining({
        baseUrl: "",
        secretValue: "",
        id: "host-subscription",
      }),
    ),
  );
});

it("lets the server allocate an independent credential for a new model", async () => {
  const save = vi.fn().mockResolvedValue(undefined);
  render(<ModelForm onSave={save} />);
  expect(screen.getByLabelText("Credential name")).toHaveValue("");
  expect(screen.getByLabelText("Credential name")).not.toBeRequired();
  fireEvent.change(screen.getByLabelText("Model ID"), {
    target: { value: "independent" },
  });
  fireEvent.change(screen.getByLabelText("Provider model ID"), {
    target: { value: "provider-model" },
  });
  fireEvent.change(screen.getByLabelText("API key"), {
    target: { value: "new-model-secret" },
  });
  fireEvent.click(screen.getByRole("button", { name: "Add model" }));
  await waitFor(() =>
    expect(save).toHaveBeenCalledWith(
      expect.objectContaining({
        secretName: "",
        secretValue: "new-model-secret",
      }),
    ),
  );
});
