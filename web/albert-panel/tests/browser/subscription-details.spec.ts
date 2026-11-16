import { expect, test } from "@playwright/test";
import { clientContext, readyFixture } from "./fixture";
test("expanded model details and unavailable reasons keep the mobile actions reachable", async ({
  page,
}) => {
  await page.setViewportSize({ width: 390, height: 600 });
  await readyFixture(
    page,
    clientContext({
      setup: {
        phase: "model",
        model_ready: false,
        model_configured: false,
        memory_configured: true,
        identity_required: false,
      },
    }),
  );
  const longId = "account-model-with-a-long-provider-identifier-".repeat(6);
  const unavailable =
    "This model is unavailable for this account. Choose another model or reconnect a different subscription.";
  const model = {
    slug: longId,
    name: "Account reasoning model",
    description: "A model description supplied by the provider.",
    input_modalities: ["text", "image"],
    context_window: 500000,
    reasoning_efforts: ["low", "medium", "high", "xhigh"],
    default_reasoning_effort: "medium",
    selectable: true,
    unavailable_reason: null,
    tool_mode: "code_mode_only",
  };
  await page.route("**/api/v1/openai-**", (route) =>
    route.fulfill({
      json: route.request().url().endsWith("/catalogs")
        ? {
            generation: "generation",
            revision: "snapshot",
            models: [
              model,
              {
                ...model,
                slug: "unavailable-model",
                name: "Unavailable account model",
                selectable: false,
                unavailable_reason: unavailable,
              },
            ],
          }
        : {
            connected: true,
            generation: "generation",
            phase: "connected",
            selected_model_id: null,
            selected_slug: null,
            preferred_model_id: null,
            message: null,
          },
    }),
  );
  await page.goto("/ui/chats");
  await page.getByRole("button", { name: "Subscription", exact: true }).click();
  const dialog = page.locator(".subscription-dialog");
  const radio = dialog.getByRole("radio", {
    name: model.name,
    exact: true,
  });
  await radio.check();
  const details = dialog.locator(".settings-row-disclosure").first();
  await details.locator("summary").focus();
  await page.keyboard.press("Enter");
  await expect(details).toHaveAttribute("open", "");
  await expect(details.getByText(longId, { exact: true })).toBeVisible();
  await expect(radio).toBeChecked();
  await expect(
    dialog.getByRole("radio", {
      name: "Unavailable account model",
      exact: true,
    }),
  ).toBeDisabled();
  const reason = dialog.getByText(unavailable, { exact: true });
  await reason.scrollIntoViewIfNeeded();
  await expect(reason).toBeInViewport();
  await expect(
    dialog.getByRole("button", { name: "Use selected model", exact: true }),
  ).toBeInViewport();
  await expect(dialog.locator(".subscription-footer-note")).toBeInViewport();
  const geometry = await dialog.evaluate((element) => ({
    width: element.clientWidth,
    scrollWidth: element.scrollWidth,
    height: element.clientHeight,
    scrollHeight: element.scrollHeight,
  }));
  expect(geometry.scrollWidth).toBeLessThanOrEqual(geometry.width);
  expect(geometry.scrollHeight).toBeLessThanOrEqual(geometry.height);
});
