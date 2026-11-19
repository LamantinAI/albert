import { expect, test } from "@playwright/test";
import { policy, setup, snapshot } from "./composer-support";
test("menu selects revision-bound skills and keeps plus on the left", async ({
  page,
}) => {
  const fixture = await setup(page, false);
  const description = `${policy.skills[0].description}. ${"Compare references, verify details, and summarize the findings. ".repeat(5).trim()}`;
  await page.route("**/api/v1/composer", (route) =>
    route.fulfill({
      json: {
        ...policy,
        speech_available: false,
        skills: [
          { ...policy.skills[0], description },
          ...policy.skills.slice(1),
        ],
      },
    }),
  );
  await page.goto("/ui/chats/session-1");
  await page
    .getByRole("button", { name: "Add to message", exact: true })
    .click();
  await snapshot(page, "14-composer-menu");
  await page
    .getByRole("button", { name: /Skills Choose instructions/ })
    .click();
  await page.getByRole("textbox", { name: "Search skills" }).fill("research");
  const preview = page.getByText(description, { exact: true });
  await expect(preview).toBeVisible();
  expect(
    await preview.evaluate((element) => ({
      clipped: element.scrollWidth > element.clientWidth,
      singleLine:
        element.clientHeight <=
        Math.ceil(parseFloat(getComputedStyle(element).lineHeight)),
    })),
  ).toEqual({ clipped: true, singleLine: true });
  await snapshot(page, "15-composer-skills");
  await page.getByRole("button", { name: /Research Read sources/ }).click();
  await expect(
    page.getByRole("button", { name: "Remove skill Research" }),
  ).toBeVisible();
  await page
    .getByRole("textbox", { name: "Message Albert" })
    .fill("Prepare a report");
  const plus = await page
      .getByRole("button", { name: "Add to message", exact: true })
      .boundingBox(),
    field = await page
      .getByRole("textbox", { name: "Message Albert" })
      .boundingBox();
  expect(plus!.x).toBeLessThan(field!.x);
  await page.getByRole("button", { name: "Send", exact: true }).click();
  await expect
    .poll(
      () =>
        fixture.requests.filter(
          (request) =>
            request.path.endsWith("/messages") && request.method === "POST",
        ).length,
    )
    .toBe(1);
  expect(
    fixture.requests.find(
      (request) =>
        request.path.endsWith("/messages") && request.method === "POST",
    )!.body,
  ).toMatchObject({ skills: [{ id: "research", revision: "r1" }] });
});

test("Escape restores focus and compact invokes a typed operation", async ({
  page,
}) => {
  const fixture = await setup(page, false);
  let compactBody: unknown;
  await page.route("**/api/v1/sessions/*/compactions", (route) => {
    compactBody = route.request().postDataJSON();
    return route.fulfill({
      json: {
        client_message_id: (compactBody as { client_request_id: string })
          .client_request_id,
        message_ids: ["compact-receipt"],
        duplicate: false,
        attempt_id: null,
      },
    });
  });
  await page.goto("/ui/chats/session-1");
  const add = page.getByRole("button", { name: "Add to message", exact: true });
  await add.click();
  await page.keyboard.press("Escape");
  await expect(add).toBeFocused();
  await add.click();
  await page.getByRole("button", { name: /Commands Conversation/ }).click();
  await page
    .getByRole("button", { name: /Compact conversation Summarize/ })
    .click();
  await page
    .getByRole("textbox", { name: "Instructions (optional)" })
    .fill("Keep decisions");
  await page.getByRole("button", { name: "Compact", exact: true }).click();
  await expect
    .poll(() => compactBody)
    .toMatchObject({ instructions: "Keep decisions" });
  expect(
    fixture.requests.filter(
      (request) =>
        request.path.endsWith("/messages") && request.method === "POST",
    ),
  ).toHaveLength(0);
});

for (const [width, height] of [
  [1440, 900],
  [1920, 1080],
  [390, 844],
])
  test(`composer layout ${width}x${height}`, async ({ page }) => {
    await page.setViewportSize({ width, height });
    await setup(page);
    await page.goto("/ui/chats");
    await expect(
      page.getByRole("button", { name: "Record voice message" }),
    ).toBeVisible();
    await page
      .getByRole("button", { name: "Add to message", exact: true })
      .click();
    await snapshot(page, `18-composer-${width}`);
    const box = await page
      .getByRole("dialog", { name: "Add to message" })
      .boundingBox();
    expect(box!.x).toBeGreaterThanOrEqual(0);
    expect(box!.x + box!.width).toBeLessThanOrEqual(width);
    expect(box!.y).toBeGreaterThanOrEqual(0);
    expect(
      await page.evaluate(
        () => document.documentElement.scrollWidth > innerWidth,
      ),
    ).toBe(false);
  });
