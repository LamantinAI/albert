import type { Page } from "@playwright/test";
import { expect } from "@playwright/test";
import { clientContext, readyFixture } from "./fixture";
export async function authorizeFixture(page: Page) {
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
  let starts = 0,
    cancels = 0;
  const attempt = {
    id: "same-attempt",
    phase: "awaiting_callback",
    authorize_url: "https://auth.example.test/authorize?state=synthetic",
    redirect_uri: "http://localhost:1455/auth/callback",
    expires_at: "2099-01-01T12:00:00Z",
    credentials_saved: false,
    message: null,
  };
  await page.route("**/api/v1/openai-**", (route) => {
    const path = new URL(route.request().url()).pathname;
    if (path.endsWith("/openai-subscription"))
      return route.fulfill({
        json: {
          connected: false,
          generation: null,
          phase: "disconnected",
          selected_model_id: null,
          selected_slug: null,
          preferred_model_id: null,
          message: null,
        },
      });
    if (path.endsWith("/openai-login-attempts")) starts++;
    if (path.endsWith("/cancellations")) {
      cancels++;
      return route.fulfill({ json: { ...attempt, phase: "cancelled" } });
    }
    return route.fulfill({ json: attempt });
  });
  await page.goto("/ui/chats");
  await page.getByRole("button", { name: "Subscription", exact: true }).click();
  const dialog = page.locator(".subscription-dialog");
  await expect(
    dialog.getByRole("link", { name: "Open OpenAI sign-in", exact: true }),
  ).toBeVisible();
  return { dialog, starts: () => starts, cancels: () => cancels };
}
