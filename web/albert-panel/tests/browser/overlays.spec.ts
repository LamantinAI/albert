import { expect, test } from "@playwright/test";

import { readyFixture } from "./fixture";

test("rename uses the panel dialog and leaves title limits to the existing server contract", async ({
  page,
}) => {
  const f = await readyFixture(page);
  await page.goto("/ui/chats/session-1");
  await page
    .getByRole("button", { name: "Actions for Research notes", exact: true })
    .click();
  await page.getByRole("menuitem", { name: "Rename", exact: true }).click();
  const dialog = page.getByRole("dialog", { name: "Rename conversation" });
  const title = `A descriptive title ${"x".repeat(220)}`;
  await dialog.getByLabel("Conversation title").fill(title);
  await dialog.getByRole("button", { name: "Save name", exact: true }).click();
  await expect(dialog).not.toBeVisible();
  expect(f.requests.filter((r) => r.method === "PATCH")).toHaveLength(1);
  expect(f.requests.find((r) => r.method === "PATCH")?.body).toMatchObject({
    title,
  });
});

test("temporary End uses a scoped modal with cancel, Escape and focus restoration", async ({
  page,
}) => {
  const fixture = await readyFixture(page);
  let nativeDialogs = 0;
  page.on("dialog", (dialog) => {
    nativeDialogs++;
    void dialog.dismiss();
  });
  fixture.temporary();
  await page.goto("/ui/chats/session-1");
  const options = page.getByRole("button", {
    name: "Actions for Research notes",
    exact: true,
  });
  async function open() {
    await options.click();
    await page.getByRole("menuitem", { name: "Delete", exact: true }).click();
    await expect(
      page.getByRole("dialog", { name: "End temporary chat?" }),
    ).toBeVisible();
  }
  await open();
  const modal = page.getByRole("dialog", { name: "End temporary chat?" });
  await expect(modal.getByRole("button", { name: "Keep chat" })).toBeFocused();
  await page.keyboard.press("Escape");
  await expect(modal).not.toBeVisible();
  await expect(options).toBeFocused();
  await open();
  await modal.getByRole("button", { name: "Keep chat" }).click();
  expect(fixture.requests.filter((r) => r.method === "PATCH")).toHaveLength(0);
  await open();
  await modal.getByRole("button", { name: "End chat", exact: true }).click();
  await expect(page).toHaveURL(/\/ui\/chats$/);
  expect(fixture.requests.filter((r) => r.method === "PATCH")).toHaveLength(1);
  expect(nativeDialogs).toBe(0);
});

test("connection selector is an in-page listbox supporting keyboard selection and Escape", async ({
  page,
}) => {
  await readyFixture(page);
  await page.route("**/api/v1/configuration", (route) =>
    route.fulfill({
      json: { items: ["connectors", "connector:http", "connector:telegram"] },
    }),
  );
  await page.route("**/api/v1/configuration/*", (route) =>
    route.fulfill({
      json: {
        path: "connectors",
        content: "[connectors]\ndir='connectors'\n",
        revision: "r1",
        redacted: false,
      },
    }),
  );
  await page.goto("/ui/settings/connections");
  const control = page.getByRole("combobox", { name: "Connection document" });
  await control.click();
  await expect(page.getByRole("listbox")).toBeVisible();
  await page.keyboard.press("ArrowDown");
  await page.keyboard.press("Enter");
  await expect(control).toContainText("http");
  await expect(control).toBeFocused();
  await control.click();
  await page.keyboard.press("Escape");
  await expect(page.getByRole("listbox")).not.toBeVisible();
  await expect(control).toBeFocused();
  await control.click();
  await page.keyboard.press("t");
  await page.keyboard.press("Enter");
  await expect(control).toContainText("telegram");
});
