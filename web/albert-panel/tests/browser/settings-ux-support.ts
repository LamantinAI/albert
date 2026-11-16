import type { Page } from "@playwright/test";
import { readyFixture } from "./fixture";
export async function documents(page: Page) {
  await readyFixture(page);
  const writes: { path: string; body: Record<string, unknown> }[] = [];
  let fail = false;
  await page.route("**/api/v1/prompts/**", (route) => {
    const id = new URL(route.request().url()).pathname.split("/").at(-1)!;
    const content =
      id === "soul"
        ? "# Persona\nOriginal instructions"
        : "# Operating instructions\nUse connected tools";
    if (route.request().method() === "PUT") {
      const body = route.request().postDataJSON();
      writes.push({ path: id, body });
      return route.fulfill({
        json: {
          path: id,
          content: body.content,
          revision: "r2",
          redacted: false,
        },
      });
    }
    return route.fulfill({
      json: { path: id, content, revision: "r1", redacted: false },
    });
  });
  await page.route("**/api/v1/configuration/**", (route) => {
    const path = new URL(route.request().url()).pathname;
    const id = decodeURIComponent(
      path.split("/").at(path.endsWith("/drafts") ? -2 : -1)!,
    );
    const content =
      id === "albert" ? "timezone = 'UTC'" : "# Original connector document";
    if (fail && id === "albert" && !path.endsWith("/drafts")) {
      fail = false;
      return route.fulfill({
        status: 503,
        json: {
          error: {
            code: "unavailable",
            message: "Configuration temporarily unavailable.",
          },
        },
      });
    }
    return route.fulfill({
      json: {
        path: id,
        content,
        revision: "r1",
        redacted: false,
        ...(path.endsWith("/drafts")
          ? {
              fields: [
                {
                  path: "agent.max_tool_turns",
                  group: "General",
                  label: "Tool rounds per turn",
                  kind: "integer",
                  value: 17,
                  default_value: 17,
                  minimum: 1,
                  maximum: 100,
                  description: "Maximum tool rounds for a turn.",
                },
              ],
            }
          : {}),
      },
    });
  });
  await page.route("**/api/v1/configuration", (route) =>
    route.fulfill({
      json: { items: ["connectors", "connector:browser"], revision: "r1" },
    }),
  );
  await page.route("**/api/v1/configuration-validations", (route) =>
    route.fulfill({
      json: { valid: true, issues: [], requires_restart: false },
    }),
  );
  return {
    writes,
    failNext: () => {
      fail = true;
    },
  };
}
