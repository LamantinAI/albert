import { expect, test } from "@playwright/test";
import { account, clientContext } from "./fixture";
import { approval } from "./approval-fixture";

test("owner reviews native approval summaries and explicitly approves or declines; users cannot open the queue", async ({
  page,
}) => {
  let owner = true,
    reads = 0;
  let items = [
    approval({ id: "opaque-first" }),
    approval({
      id: "opaque-second",
      title: "Update records",
      action: "Update Notes",
      effects: ["Affected objects: drafts"],
    }),
  ];
  const decisions: { id: string; approve: boolean }[] = [];
  await page.route("**/api/v1/**", async (route) => {
    const path = new URL(route.request().url()).pathname;
    if (path.endsWith("/sessions"))
      return route.fulfill({
        json: { items: [], has_more: false, next_cursor: null },
      });
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
      return route.fulfill({ json: { items, warnings: [] } });
    }
    if (path.endsWith("/decisions")) {
      const id = path.split("/").at(-2)!,
        { approve } = route.request().postDataJSON();
      decisions.push({ id, approve });
      const item = items.find((item) => item.id === id)!;
      items = items.filter((item) => item.id !== id);
      return route.fulfill({
        json: {
          ...item,
          state: approve ? "committed" : "rejected",
          can_decide: false,
        },
      });
    }
    return route.fulfill({ status: 404 });
  });
  await page.goto("/ui/approvals");
  await expect(
    page.getByRole("heading", { name: "Delete records", exact: true }),
  ).toBeVisible();
  expect(decisions).toHaveLength(0);
  await page
    .getByRole("button", { name: "Review", exact: true })
    .first()
    .click();
  let review = page.getByRole("dialog", {
    name: "Protected action",
    exact: true,
  });
  await expect(review).toContainText("irreversible");
  await expect(
    review.getByText("Affected objects: food_log", { exact: true }),
  ).toBeVisible();
  await expect(page.getByText("must-not-render", { exact: true })).toHaveCount(
    0,
  );
  await review.getByRole("button", { name: "Approve", exact: true }).click();
  await expect(review).toContainText("Completed. The change was recorded.");
  await review.getByRole("button", { name: "Close", exact: true }).click();
  await page.getByRole("button", { name: "Review", exact: true }).click();
  review = page.getByRole("dialog", { name: "Protected action", exact: true });
  await review.getByRole("button", { name: "Decline", exact: true }).click();
  await expect(review).toContainText("Declined. Permission was not granted.");
  await review.getByRole("button", { name: "Close", exact: true }).click();
  await expect(
    page.getByRole("heading", {
      name: "No decisions are waiting.",
      exact: true,
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
  const item = approval({ id: "uncertain-action" });
  await page.route("**/api/v1/**", (route) => {
    const path = new URL(route.request().url()).pathname;
    if (path.endsWith("/sessions"))
      return route.fulfill({
        json: { items: [], has_more: false, next_cursor: null },
      });
    if (path.endsWith("/context"))
      return route.fulfill({ json: clientContext() });
    if (path.endsWith("/decisions")) {
      pending = false;
      return route.fulfill({
        status: 503,
        json: { error: { message: "Approval outcome is unknown." } },
      });
    }
    if (path.endsWith("/approvals"))
      return route.fulfill({
        json: { items: pending ? [item] : [], warnings: [] },
      });
    if (path.endsWith("/approvals/uncertain-action"))
      return route.fulfill({ json: item });
    return route.fulfill({ status: 404 });
  });
  await page.goto("/ui/approvals");
  await page.getByRole("button", { name: "Review", exact: true }).click();
  let review = page.getByRole("dialog", {
    name: "Protected action",
    exact: true,
  });
  await review.getByRole("button", { name: "Approve", exact: true }).click();
  await expect(review).toContainText("The outcome is not confirmed.");
  await expect(
    review.getByRole("button", { name: "Approve", exact: true }),
  ).toHaveCount(0);
  await review.getByRole("button", { name: "Close", exact: true }).click();
  await page.reload();
  await expect(
    page.getByRole("heading", {
      name: "No decisions are waiting.",
      exact: true,
    }),
  ).toHaveCount(0);
  await page.getByRole("button", { name: "Review", exact: true }).click();
  review = page.getByRole("dialog", { name: "Protected action", exact: true });
  await expect(review).toContainText("The outcome is not confirmed.");
});
