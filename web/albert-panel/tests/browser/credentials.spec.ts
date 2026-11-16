import { expect, test } from "@playwright/test";
import { credentialFixture, openCredentials } from "./credential-fixture";
import { browserStorage, sentinel } from "./credentials-support";
test("manual handoff keeps the chat draft, masks the value and writes only through its own endpoint", async ({
  page,
}) => {
  await page.clock.install();
  const fixture = await credentialFixture(page);
  await page.goto("/ui/chats");
  const draft = page.getByRole("textbox", { name: "Message Albert" });
  await draft.fill("Keep this chat draft");
  await openCredentials(page);
  const panel = page.getByRole("region", { name: "Server credential" });
  await expect(panel).toBeVisible();
  await panel.getByLabel("Name", { exact: true }).fill("ACTUAL_TOKEN");
  const value = panel.getByLabel("Credential value");
  await value.fill(sentinel);
  await expect(value).toHaveAttribute("type", "password");
  await panel.getByRole("button", { name: "Show secret" }).click();
  await expect(value).toHaveAttribute("type", "text");
  await expect(value).toBeFocused();
  await panel.getByRole("button", { name: "Hide secret" }).click();
  await expect(value).toHaveAttribute("type", "password");
  expect(await value.getAttribute("value")).toBeNull();
  await value.evaluate((node) =>
    Object.assign(window, { credentialInput: node }),
  );
  expect(await browserStorage(page)).not.toContain(sentinel);
  expect(await page.content()).not.toContain(sentinel);
  await page.clock.pauseAt(
    new Date(await page.evaluate(() => Date.now() + 100)),
  );
  await panel.getByRole("button", { name: "Save", exact: true }).click();
  await expect(panel.getByRole("status")).toContainText("ACTUAL_TOKEN saved.");
  expect(fixture.writes).toHaveLength(1);
  expect(fixture.writes[0]).toMatchObject({
    name: "ACTUAL_TOKEN",
    value: sentinel,
  });
  await expect(draft).toHaveValue("Keep this chat draft");
  expect(
    fixture.base.requests.filter(
      (request) =>
        request.method === "POST" &&
        (request.path.includes("/messages") ||
          request.path.includes("/session-starts") ||
          request.path === "/sessions"),
    ),
  ).toHaveLength(0);
  expect(await browserStorage(page)).not.toContain(sentinel);
  expect(await page.content()).not.toContain(sentinel);
  expect(
    await page.evaluate(
      () =>
        (window as unknown as { credentialInput: HTMLInputElement })
          .credentialInput.value,
    ),
  ).toBe("");
  await page.clock.runFor(999);
  await expect(panel.getByRole("status")).toContainText("ACTUAL_TOKEN saved.");
  await expect(panel).not.toHaveClass(/is-closing/);
  await page.clock.runFor(1);
  await expect(panel).toHaveClass(/is-closing/);
  await page.clock.runFor(150);
  await expect(panel).toHaveCount(0);
  await expect(draft).toHaveValue("Keep this chat draft");
  expect(fixture.writes).toHaveLength(1);
  await openCredentials(page);
  await expect(panel.getByLabel("Name", { exact: true })).toHaveValue("");
  await expect(panel).not.toHaveClass(/is-closing/);
});

test("agent request pre-fills an editable name and a lost save response is recovered without resending", async ({
  page,
}) => {
  const fixture = await credentialFixture(page, { agent: true, mode: "lost" });
  await page.goto("/ui/chats/session-1");
  const panel = page.getByRole("region", { name: "Server credential" });
  await expect(panel.getByLabel("Name", { exact: true })).toHaveValue(
    "SUGGESTED_TOKEN",
  );
  await expect(panel.getByLabel("Credential value")).toBeFocused();
  await panel.getByLabel("Name", { exact: true }).fill("RENAMED_TOKEN");
  await panel.getByLabel("Credential value").fill(sentinel);
  await panel.getByRole("button", { name: "Save", exact: true }).click();
  await expect(panel.getByRole("status")).toContainText("RENAMED_TOKEN saved.");
  expect(fixture.writes).toHaveLength(1);
  expect(fixture.writes[0].name).toBe("RENAMED_TOKEN");
  await page.reload();
  await expect(
    page.getByRole("region", { name: "Server credential" }).getByRole("status"),
  ).toContainText("RENAMED_TOKEN saved.");
  expect(fixture.writes).toHaveLength(1);
  expect(await browserStorage(page)).not.toContain(sentinel);
});

test("session navigation clears credential fields; temporary manual entry does not promote or create a chat", async ({
  page,
}) => {
  const fixture = await credentialFixture(page, { agent: true });
  await page.goto("/ui/chats/session-1");
  const value = page.getByLabel("Credential value");
  await value.fill(sentinel);
  await value.evaluate((node) =>
    Object.assign(window, { credentialInput: node }),
  );
  await page.getByRole("button", { name: "New chat", exact: true }).click();
  await expect(page.getByLabel("Credential value")).toHaveCount(0);
  expect(
    await page.evaluate(
      () =>
        (window as unknown as { credentialInput: HTMLInputElement })
          .credentialInput.value,
    ),
  ).toBe("");
  expect(await browserStorage(page)).not.toContain(sentinel);
  await page.goto("/ui/chats?new=temporary");
  await openCredentials(page);
  const panel = page.getByRole("region", { name: "Server credential" });
  await expect(panel).toContainText("independently of this temporary chat");
  await panel.getByLabel("Name", { exact: true }).fill("TEMP_TOKEN");
  await panel.getByLabel("Credential value").fill(sentinel);
  await panel.getByRole("button", { name: "Save", exact: true }).click();
  await expect(panel.getByRole("status")).toContainText("TEMP_TOKEN saved.");
  expect(
    fixture.base.requests.filter(
      (request) =>
        request.method === "POST" &&
        (request.path === "/sessions" || request.path === "/session-starts"),
    ),
  ).toHaveLength(0);
});

test("multiline paste is rejected before the browser can flatten it", async ({
  page,
}) => {
  await credentialFixture(page, { agent: true });
  await page.goto("/ui/chats/session-1");
  const value = page.getByLabel("Credential value");
  await value.evaluate((node) => {
    const clipboard = new DataTransfer();
    clipboard.setData("text/plain", "first-line\nsecond-line");
    node.dispatchEvent(
      new ClipboardEvent("paste", {
        bubbles: true,
        cancelable: true,
        clipboardData: clipboard,
      }),
    );
  });
  await expect(page.getByRole("alert")).toContainText(
    "Multiline input was not pasted",
  );
  await expect(value).toHaveValue("");
});

test("members cannot open credential controls", async ({ page }) => {
  await credentialFixture(page, { owner: false });
  await page.goto("/ui/chats/session-1");
  await page
    .getByRole("button", { name: "Add to message", exact: true })
    .click();
  await page
    .getByRole("button", { name: /Commands Conversation actions/ })
    .click();
  await expect(
    page.getByRole("button", { name: /^Credentials Add or update/ }),
  ).toHaveCount(0);
  await expect(
    page.getByRole("region", { name: "Server credential" }),
  ).toHaveCount(0);
});

test("a dedicated credential event opens the form after a turn has ended without reloading message history", async ({
  page,
}) => {
  await page.addInitScript(() => {
    const sources: EventTarget[] = [];
    Object.assign(window, { credentialSources: sources });
    class Source extends EventTarget {
      onopen: ((event: Event) => void) | null = null;
      onmessage: ((event: MessageEvent) => void) | null = null;
      constructor(url: string) {
        super();
        if (!url.includes("/operator/")) sources.push(this);
        queueMicrotask(() => this.onopen?.(new Event("open")));
      }
      close() {}
    }
    Object.defineProperty(window, "EventSource", { value: Source });
  });
  const fixture = await credentialFixture(page);
  await page.goto("/ui/chats/session-1");
  await expect(
    page.getByRole("textbox", { name: "Message Albert" }),
  ).toBeEnabled();
  await expect(
    page.getByRole("region", { name: "Server credential" }),
  ).toHaveCount(0);
  const before = fixture.base.requests.filter(
    (request) => request.method === "GET" && request.path.endsWith("/messages"),
  ).length;
  fixture.create("late-request", "session-1", "LATER_TOKEN", "agent");
  await page.evaluate(() => {
    const sources = (window as unknown as { credentialSources: EventTarget[] })
      .credentialSources;
    sources.at(-1)!.dispatchEvent(
      new MessageEvent("credentials", {
        data: JSON.stringify({
          id: "epoch-1/session-1/77",
          session_id: "session-1",
          runtime_epoch: "epoch-1",
          kind: "credentials",
          attempt_id: null,
          data: { request_id: "late-request" },
        }),
      }),
    );
  });
  const panel = page.getByRole("region", { name: "Server credential" });
  await expect(panel.getByLabel("Name", { exact: true })).toHaveValue(
    "LATER_TOKEN",
  );
  expect(
    fixture.base.requests.filter(
      (request) =>
        request.method === "GET" && request.path.endsWith("/messages"),
    ).length,
  ).toBe(before);
});
