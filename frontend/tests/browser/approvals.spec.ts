import { expect, test } from "@playwright/test";

import type { components } from "../../src/api/schema";
import { account, clientContext } from "./fixture";

test("owner reviews native approval summaries and explicitly approves or declines; users cannot open the queue", async ({
  page,
}) => {
  let owner = true;
  let reads = 0;
  let items: components["schemas"]["ApprovalView"][] = [
    {
      id: "opaque-first",
      connector: "data",
      state: "pending",
      expires_at: 2500000000,
      summary: {
        resource: "Nutrition",
        action: "delete_records",
        objects: ["food_log"],
        operation_hash: "must-not-render",
      },
    },
    {
      id: "opaque-second",
      connector: "data",
      state: "pending",
      expires_at: 2500000000,
      summary: {
        resource: "Notes",
        action: "update_records",
        objects: ["drafts"],
      },
    },
  ];
  const decisions: { id: string; approve: boolean }[] = [];
  await page.route("**/api/v1/**", async (route) => {
    const path = new URL(route.request().url()).pathname;
    if (path.endsWith("/context"))
      return route.fulfill({
        json: owner
          ? clientContext()
          : clientContext({
              account: { ...account, role: "user" },
              caps: {
                owner: false,
                chat: true,
                manage_users: false,
                manage_models: false,
                manage_skills: false,
                manage_config: false,
              },
            }),
      });
    if (path.endsWith("/approvals")) {
      reads++;
      return route.fulfill({ json: { items } });
    }
    if (path.endsWith("/decisions")) {
      const id = path.split("/").at(-2)!;
      const { approve } = route.request().postDataJSON();
      decisions.push({ id, approve });
      const item = items.find((item) => item.id === id)!;
      items = items.filter((item) => item.id !== id);
      return route.fulfill({
        json: { ...item, state: approve ? "committed" : "rejected" },
      });
    }
    return route.fulfill({ status: 404 });
  });
  await page.goto("/ui/approvals");
  await expect(
    page.getByRole("heading", { name: "Delete records", exact: true }),
  ).toBeVisible();
  await expect(
    page.getByText("Affected objects: food_log", { exact: true }),
  ).toBeVisible();
  await expect(page.getByText("must-not-render", { exact: true })).toHaveCount(
    0,
  );
  expect(decisions).toHaveLength(0);
  page.once("dialog", async (dialog) => {
    expect(dialog.message()).toContain("irreversible");
    await dialog.accept();
  });
  await page
    .getByRole("button", { name: "Approve", exact: true })
    .first()
    .click();
  await expect(
    page.getByText("Approved. The change completed.", { exact: true }),
  ).toBeVisible();
  await expect(
    page.getByRole("button", { name: "Decline", exact: true }),
  ).toHaveCount(1);
  await page.getByRole("button", { name: "Decline", exact: true }).click();
  await expect(
    page.getByText("Declined. The change was not authorized.", { exact: true }),
  ).toBeVisible();
  await expect(
    page.getByRole("heading", {
      name: "No changes are waiting for your decision.",
    }),
  ).toBeVisible();
  expect(decisions).toEqual([
    { id: "opaque-first", approve: true },
    { id: "opaque-second", approve: false },
  ]);
  owner = false;
  const ownerReads = reads;
  await page.reload();
  await expect(
    page.getByRole("heading", { name: "This area is for workspace owners." }),
  ).toBeVisible();
  await expect(
    page.getByRole("link", { name: "Approvals", exact: true }),
  ).toHaveCount(0);
  expect(reads).toBe(ownerReads);
});

test("a refreshed empty queue preserves an unknown decision outcome", async ({
  page,
}) => {
  let pending = true;
  await page.route("**/api/v1/**", async (route) => {
    const path = new URL(route.request().url()).pathname;
    if (path.endsWith("/context"))
      return route.fulfill({ json: clientContext() });
    if (path.endsWith("/decisions")) {
      pending = false;
      return route.fulfill({
        status: 503,
        json: {
          error: {
            code: "unavailable",
            message: "Approval outcome is unknown.",
          },
        },
      });
    }
    return route.fulfill({
      json: {
        items: pending
          ? [
              {
                id: "opaque",
                connector: "data",
                state: "pending",
                summary: {
                  resource: "Nutrition",
                  action: "delete_records",
                  objects: ["food_log"],
                },
              },
            ]
          : [],
      },
    });
  });
  await page.goto("/ui/approvals");
  page.once("dialog", (dialog) => dialog.accept());
  await page.getByRole("button", { name: "Approve", exact: true }).click();
  await expect(
    page.getByRole("heading", {
      name: "No changes are waiting for your decision.",
    }),
  ).toBeVisible();
  await expect(page.getByRole("alert")).toContainText(
    "Approval outcome is unknown.",
  );
});
