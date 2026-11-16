import { expect, test } from "@playwright/test";
import type { Page } from "@playwright/test";
import { agentFixture } from "./agent-settings-support";
async function openMemory(page: Page) {
  const group = page.locator("#memory");
  if ((await group.getAttribute("open")) === null)
    await group.locator("summary").click();
  return group;
}
async function httpMemory(page: Page) {
  await openMemory(page);
  await page.getByRole("combobox", { name: "Memory backend" }).click();
  await page.getByRole("option", { name: "Kaeru over MCP" }).click();
  await page
    .getByLabel("Server URL", { exact: true })
    .fill("https://example.com/mcp");
}
test("one Save persists both drafts and Raw switching keeps the write-only token", async ({
  page,
}) => {
  const fixture = await agentFixture(page);
  await page.goto("/ui/settings");
  await httpMemory(page);
  await page
    .getByLabel("Bearer token", { exact: true })
    .fill("fixture-only-bearer");
  const timezone = page.getByRole("textbox", { name: "Timezone", exact: true });
  await expect(timezone).toBeEnabled();
  await timezone.fill("Europe/Paris");
  await page.getByRole("button", { name: "Raw", exact: true }).click();
  await expect(
    page.getByRole("textbox", { name: "Agent configuration" }),
  ).toHaveValue(/Europe\/Paris/);
  await expect(
    page.getByRole("textbox", { name: "Agent configuration" }),
  ).not.toHaveValue(/fixture-only-bearer/);
  await page.getByRole("button", { name: "Fields", exact: true }).click();
  await openMemory(page);
  await expect(page.getByLabel("Bearer token", { exact: true })).toHaveValue(
    "fixture-only-bearer",
  );
  await page.getByRole("button", { name: "Save changes", exact: true }).click();
  await expect(
    page.getByText("Saved · restart to apply", { exact: true }),
  ).toBeVisible();
  const writes = fixture.requests.filter((x) => x.method === "PUT");
  expect(writes).toHaveLength(1);
  expect(writes[0].path).toBe("/api/v1/memory");
  expect(writes[0].body.document.content).toContain("Europe/Paris");
  expect(writes[0].body.document.content).toContain("# Keep this comment");
  expect(writes[0].body.token_value).toBe("fixture-only-bearer");
  expect(fixture.document().content).not.toContain("fixture-only-bearer");
  await expect(
    page.getByRole("button", { name: "Validate changes" }),
  ).toHaveCount(0);
  await expect(
    page.getByRole("button", { name: "Save memory settings" }),
  ).toHaveCount(0);
});
test("invalid stdio text survives Raw switching without locking the other sections", async ({
  page,
}) => {
  await agentFixture(page);
  await page.goto("/ui/settings");
  await httpMemory(page);
  await page.getByRole("combobox", { name: "Transport", exact: true }).click();
  await page.getByRole("option", { name: "Local stdio process" }).click();
  await page.getByLabel("Command", { exact: true }).fill("kaeru");
  await page.getByLabel("Environment references").fill("unfinished");
  await page.getByRole("button", { name: "Raw", exact: true }).click();
  await page.getByRole("button", { name: "Fields", exact: true }).click();
  await openMemory(page);
  await expect(page.getByLabel("Environment references")).toHaveValue(
    "unfinished",
  );
  await expect(
    page.getByText("Use a unique VARIABLE=CREDENTIAL_REFERENCE on each line."),
  ).toBeVisible();
  await expect(
    page.getByRole("button", { name: "Save changes", exact: true }),
  ).toBeDisabled();
  await expect(
    page.getByRole("spinbutton", { name: "Tool rounds per turn", exact: true }),
  ).toBeEnabled();
  await page
    .getByLabel("Environment references")
    .fill("TOKEN=STORED_CREDENTIAL");
  await expect(
    page.getByRole("button", { name: "Save changes", exact: true }),
  ).toBeEnabled();
});
test("display unit selection does not create an override and decimal edits store exact native units", async ({
  page,
}) => {
  const fixture = await agentFixture(page);
  await page.goto("/ui/settings");
  await expect(
    page.getByRole("spinbutton", { name: "Tool rounds per turn", exact: true }),
  ).toHaveValue("8");
  await expect(
    page.getByRole("switch", { name: "Live progress", exact: true }),
  ).toHaveAttribute("aria-checked", "true");
  await page.getByText("Skills & reflection", { exact: true }).click();
  const interval = page.getByRole("spinbutton", {
    name: "Reflection interval",
    exact: true,
  });
  await expect(interval).toHaveValue("6");
  await page
    .getByRole("combobox", { name: "Unit for Reflection interval (seconds)" })
    .click();
  await page.getByRole("option", { name: "Minutes", exact: true }).click();
  await expect(interval).toHaveValue("360");
  await expect(
    page.getByRole("button", { name: "Save changes", exact: true }),
  ).toBeDisabled();
  expect(fixture.requests.filter((x) => x.method === "PUT")).toHaveLength(0);
  await interval.fill("1.5");
  await page.getByRole("button", { name: "Save changes", exact: true }).click();
  await expect(page.getByText("Changes saved", { exact: true })).toBeVisible();
  expect(fixture.document().content).toContain("reflection.period_secs=90");
});
test("lost Memory save response retains the secret until explicit review and discard", async ({
  page,
}) => {
  const fixture = await agentFixture(page);
  await page.goto("/ui/settings");
  await httpMemory(page);
  await page
    .getByLabel("Bearer token", { exact: true })
    .fill("fixture-only-bearer");
  fixture.loseNextSave();
  await page.getByRole("button", { name: "Save changes", exact: true }).click();
  await expect(page.getByText(/save outcome needs review/)).toBeVisible();
  await expect(page.getByLabel("Bearer token", { exact: true })).toHaveValue(
    "fixture-only-bearer",
  );
  await page
    .getByRole("button", { name: "Review saved settings", exact: true })
    .click();
  await expect(
    page.getByText("Currently saved", { exact: true }),
  ).toBeVisible();
  await expect(page.getByLabel("Bearer token", { exact: true })).toHaveValue(
    "fixture-only-bearer",
  );
  expect(
    fixture.requests.filter(
      (x) => x.path.endsWith("/memory") && x.method === "PUT",
    ),
  ).toHaveLength(1);
  await page
    .getByRole("button", { name: "Load saved settings", exact: true })
    .click();
  await page
    .getByRole("dialog", { name: "Load saved settings?" })
    .getByRole("button", { name: "Load saved settings", exact: true })
    .click();
  await expect(page.getByLabel("Bearer token", { exact: true })).toHaveValue(
    "",
  );
});
test("loading saved settings cancels invalid edits and updates the revision used for restart", async ({
  page,
}) => {
  const fixture = await agentFixture(page);
  await page.goto("/ui/settings");
  const rounds = page.getByRole("spinbutton", {
    name: "Tool rounds per turn",
    exact: true,
  });
  await rounds.fill("1.5");
  fixture.externalChange("Europe/Paris");
  await page
    .getByRole("button", { name: "Review saved settings", exact: true })
    .click();
  await page
    .getByRole("button", { name: "Load saved settings", exact: true })
    .click();
  await page
    .getByRole("dialog", { name: "Load saved settings?" })
    .getByRole("button", { name: "Load saved settings", exact: true })
    .click();
  await expect(rounds).toHaveValue("8");
  await expect(
    page.getByRole("textbox", { name: "Timezone", exact: true }),
  ).toHaveValue("Europe/Paris");
  await expect(
    page.getByRole("button", { name: "Save changes", exact: true }),
  ).toBeDisabled();
  await page.getByText("More actions", { exact: true }).click();
  await page
    .getByRole("button", { name: "Restart Albert", exact: true })
    .click();
  await page
    .getByRole("dialog", { name: "Restart Albert?" })
    .getByRole("button", { name: "Restart Albert", exact: true })
    .click();
  await expect
    .poll(
      () =>
        fixture.requests.filter(
          (x) => x.path.endsWith("/applications") && x.method === "POST",
        ).length,
    )
    .toBe(1);
  expect(
    fixture.requests.find(
      (x) => x.path.endsWith("/applications") && x.method === "POST",
    )!.body.revision,
  ).toBe(
    fixture.requests.find(
      (x) => x.path.endsWith("/applications") && x.method === "POST",
    )!.body.document_id === "albert"
      ? fixture.document().revision
      : fixture.memory().revision,
  );
});
test("a lost second restart is recovered with its own request, not the first receipt", async ({
  page,
}) => {
  const fixture = await agentFixture(page);
  await page.goto("/ui/settings");
  await page.getByText("More actions", { exact: true }).click();
  async function restart() {
    await page
      .getByRole("button", { name: "Restart Albert", exact: true })
      .click();
    await page
      .getByRole("dialog", { name: "Restart Albert?" })
      .getByRole("button", { name: "Restart Albert", exact: true })
      .click();
  }
  await restart();
  await expect(
    page.getByText("Saved settings applied", { exact: true }).first(),
  ).toBeVisible();
  fixture.loseNextRestart();
  await restart();
  await expect(
    page.getByRole("button", { name: "Check restart outcome" }),
  ).toBeVisible();
  await page.getByRole("button", { name: "Check restart outcome" }).click();
  await expect
    .poll(
      () =>
        fixture.requests.filter(
          (x) => x.path.endsWith("/applications") && x.method === "POST",
        ).length,
    )
    .toBe(3);
  const requests = fixture.requests
    .filter((x) => x.path.endsWith("/applications") && x.method === "POST")
    .map((x) => x.body);
  expect(requests[1].client_request_id).not.toBe(requests[0].client_request_id);
  expect(requests[2]).toEqual(requests[1]);
});
test("saved memory initialization failure exposes a retry without resaving the configuration", async ({
  page,
}) => {
  const fixture = await agentFixture(page);
  await page.goto("/ui/settings");
  await httpMemory(page);
  fixture.failInitialization();
  await page.getByRole("button", { name: "Save changes", exact: true }).click();
  await expect(
    page.getByRole("button", { name: "Retry memory initialization" }),
  ).toBeVisible();
  await page
    .getByRole("button", { name: "Retry memory initialization" })
    .click();
  await expect(
    page.getByText("Memory initialized", { exact: true }),
  ).toBeVisible();
  expect(fixture.requests.filter((x) => x.method === "PUT")).toHaveLength(1);
  expect(
    fixture.requests.filter((x) => x.path.endsWith("/initializations")),
  ).toHaveLength(1);
});
test("mobile scroll stays below the opaque toolbar without horizontal overflow", async ({
  page,
}) => {
  await page.setViewportSize({ width: 390, height: 844 });
  await agentFixture(page);
  await page.goto("/ui/settings");
  await openMemory(page);
  await page.locator("#memory").scrollIntoViewIfNeeded();
  const geometry = await page.evaluate(() => {
    const bar = document.querySelector(".workspace-tools")!,
      content = document.querySelector(".workspace-content")!;
    return {
      bottom: bar.getBoundingClientRect().bottom,
      contentTop: content.getBoundingClientRect().top,
      background: getComputedStyle(bar).backgroundColor,
      scroll: document.documentElement.scrollWidth,
      width: innerWidth,
    };
  });
  expect(geometry.contentTop).toBeGreaterThanOrEqual(geometry.bottom);
  expect(geometry.background).not.toBe("rgba(0, 0, 0, 0)");
  expect(geometry.scroll).toBeLessThanOrEqual(geometry.width);
});

test("discarding a draft cancels a delayed projection instead of replaying it into saved settings", async ({
  page,
}) => {
  const fixture = await agentFixture(page);
  let release!: () => void,
    pending = false;
  const gate = new Promise<void>((done) => {
    release = done;
  });
  await page.route("**/api/v1/configuration/albert/drafts", async (route) => {
    const body = route.request().postDataJSON();
    if (
      body.changes.some(
        (change: { path: string }) => change.path === "agent.max_tool_turns",
      )
    ) {
      pending = true;
      await gate;
      return route
        .fulfill({
          json: {
            content: body.content.replace(
              "agent.max_tool_turns=8",
              "agent.max_tool_turns=11",
            ),
            revision: body.revision,
            fields: [],
          },
        })
        .catch(() => {});
    }
    return route.fallback();
  });
  await page.goto("/ui/settings");
  const rounds = page.getByRole("spinbutton", {
    name: "Tool rounds per turn",
    exact: true,
  });
  await rounds.fill("11");
  await expect.poll(() => pending).toBe(true);
  await page
    .getByRole("button", { name: "Review saved settings", exact: true })
    .click();
  await page
    .getByRole("button", { name: "Load saved settings", exact: true })
    .click();
  await page
    .getByRole("dialog", { name: "Load saved settings?" })
    .getByRole("button", { name: "Load saved settings", exact: true })
    .click();
  release();
  await expect(rounds).toHaveValue("8");
  await expect(
    page.getByRole("button", { name: "Save changes", exact: true }),
  ).toBeDisabled();
  expect(fixture.requests.filter((x) => x.method === "PUT")).toHaveLength(0);
});

test("raw and dedicated memory edits conflict without clearing either draft", async ({
  page,
}) => {
  const fixture = await agentFixture(page);
  await page.goto("/ui/settings");
  await httpMemory(page);
  await page
    .getByLabel("Bearer token", { exact: true })
    .fill("fixture-only-bearer");
  await page.getByRole("button", { name: "Raw", exact: true }).click();
  const raw = page.getByRole("textbox", { name: "Agent configuration" });
  await raw.fill(
    fixture
      .document()
      .content.replace('memory.backend="embedded"', 'memory.backend="mcp"'),
  );
  await page.getByRole("button", { name: "Save changes", exact: true }).click();
  await expect(
    page.getByText(
      "Memory was edited in both the document and Memory section",
      { exact: false },
    ),
  ).toBeVisible();
  expect(fixture.document().content).toContain('memory.backend="embedded"');
  await raw.fill(fixture.document().content);
  await page.getByRole("button", { name: "Fields", exact: true }).click();
  await openMemory(page);
  await expect(page.getByLabel("Bearer token", { exact: true })).toHaveValue(
    "fixture-only-bearer",
  );
});
