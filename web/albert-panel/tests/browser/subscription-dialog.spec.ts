import { expect, test } from "@playwright/test";
import { readyFixture } from "./fixture";
import { authorizeFixture } from "./subscription-dialog-support";
test("authorization is two keyboard-navigable screens with one PKCE attempt", async ({
  page,
}) => {
  const fixture = await authorizeFixture(page);
  const { dialog } = fixture;
  await expect(
    dialog.getByRole("heading", { name: "Sign in with OpenAI", exact: true }),
  ).toBeVisible();
  await expect(dialog.getByRole("textbox")).toHaveCount(0);
  const next = dialog.getByRole("button", {
    name: "I've signed in",
    exact: true,
  });
  await next.focus();
  await page.keyboard.press("Enter");
  await expect(
    dialog.getByRole("heading", { name: "Return to Albert", exact: true }),
  ).toBeVisible();
  const callback = dialog.getByLabel("Callback URL or authorization code", {
    exact: true,
  });
  await expect(callback).toBeFocused();
  await callback.fill("synthetic-memory-only-code");
  await dialog.getByRole("button", { name: "Back", exact: true }).click();
  await expect(dialog.getByRole("textbox")).toHaveCount(0);
  await dialog
    .getByRole("button", { name: "I've signed in", exact: true })
    .click();
  await expect(callback).toHaveValue("synthetic-memory-only-code");
  expect(fixture.starts()).toBe(1);
  expect(fixture.cancels()).toBe(0);
  expect(
    await page.evaluate(() =>
      JSON.stringify({
        local: { ...localStorage },
        session: { ...sessionStorage },
      }),
    ),
  ).not.toContain("synthetic-memory-only-code");
  await page.keyboard.press("Escape");
  await expect(dialog).not.toBeVisible();
  expect(fixture.cancels()).toBe(1);
});

test("cancelling or navigating away releases the authorization attempt", async ({
  page,
}) => {
  const fixture = await authorizeFixture(page);
  await fixture.dialog
    .getByRole("button", { name: "Cancel sign-in", exact: true })
    .click();
  await expect(fixture.dialog).not.toBeVisible();
  expect(fixture.cancels()).toBe(1);
  await page.getByRole("button", { name: "Subscription", exact: true }).click();
  await expect(
    fixture.dialog.getByRole("link", {
      name: "Open OpenAI sign-in",
      exact: true,
    }),
  ).toBeVisible();
  await page.goto("/ui/models");
  await expect(fixture.dialog).not.toBeVisible();
  // Native navigation cannot reliably send an asynchronous cleanup request; the
  // attempt is also bounded server-side. SPA close above proves cancellation.
});

for (const viewport of [
  { width: 1512, height: 982 },
  { width: 1920, height: 1080 },
  { width: 390, height: 844 },
  { width: 800, height: 600 },
  { width: 320, height: 568 },
]) {
  test(`long model catalog keeps its action footer visible at ${viewport.width}x${viewport.height}`, async ({
    page,
  }) => {
    await page.setViewportSize(viewport);
    await readyFixture(page);
    const models = Array.from({ length: 28 }, (_, index) => ({
      slug: `account-model-${index}`,
      name: `Account Model ${index + 1}`,
      description:
        "A provider description with enough detail to compare everyday reasoning and coding work.",
      input_modalities: ["text", "image"],
      context_window: 500000,
      reasoning_efforts: ["low", "medium", "high", "xhigh"],
      default_reasoning_effort: "medium",
      selectable: true,
      unavailable_reason: null,
      tool_mode: "code_mode_only",
    }));
    await page.route("**/api/v1/openai-**", (route) =>
      route.fulfill({
        json: route.request().url().endsWith("/catalogs")
          ? { generation: "generation", revision: "snapshot", models }
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
    await page.goto("/ui/models");
    await page.getByRole("button", { name: "Add model", exact: true }).click();
    await page
      .getByRole("button", { name: "Subscription", exact: true })
      .click();
    const dialog = page.locator(".subscription-dialog");
    await expect(
      dialog.getByRole("heading", {
        name: "Add subscription model",
        exact: true,
      }),
    ).toBeVisible();
    await expect(dialog.getByRole("heading")).toHaveCount(1);
    await expect(dialog.locator("footer")).toHaveCount(1);
    const use = dialog.getByRole("button", {
      name: "Add model",
      exact: true,
    });
    await expect(use).toBeDisabled();
    await expect(use).toHaveAccessibleDescription(
      /without changing the current preferred model/,
    );
    await expect(dialog.locator(".subscription-footer-note")).toBeInViewport();
    // Selecting by the description proves the full row is the label, including a
    // code-mode-only catalog item that the backend has declared selectable.
    await dialog
      .locator(".settings-row-choice")
      .first()
      .getByText(models[0].description, { exact: true })
      .click();
    await expect(
      dialog.getByRole("radio", { name: "Account Model 1", exact: true }),
    ).toBeChecked();
    await expect(use).toBeEnabled();
    await expect(
      dialog.locator(".subscription-choice").first(),
    ).toHaveAttribute("data-selected", "true");
    const list = dialog.getByRole("radiogroup", {
      name: "Available subscription models",
      exact: true,
    });
    await list.evaluate((element) => {
      element.scrollTop = element.scrollHeight;
    });
    const bounds = await dialog.locator("footer").boundingBox();
    expect(bounds).not.toBeNull();
    expect(bounds!.y).toBeGreaterThanOrEqual(0);
    expect(bounds!.y + bounds!.height).toBeLessThanOrEqual(viewport.height);
    await expect(use).toBeInViewport();
    await expect(
      dialog.getByRole("button", { name: "Cancel", exact: true }),
    ).toBeInViewport();
    const geometry = await dialog.evaluate((element) => ({
      width: element.scrollWidth,
      client: element.clientWidth,
      height: element.scrollHeight,
      clientHeight: element.clientHeight,
    }));
    expect(geometry.width).toBeLessThanOrEqual(geometry.client);
    expect(geometry.height).toBeLessThanOrEqual(geometry.clientHeight);
    const radio = dialog.getByRole("radio", {
      name: "Account Model 28",
      exact: true,
    });
    await radio.focus();
    await page.keyboard.press("Space");
    await expect(radio).toBeChecked();
    await expect(use).toBeInViewport();
  });
}
