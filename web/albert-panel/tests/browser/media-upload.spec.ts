import { expect, test } from "@playwright/test";
import { message, readyFixture } from "./fixture";
import { attachments, picture } from "./media-support";
test("composer uploads real bytes and admits media-only messages without creating an empty chat", async ({
  page,
}) => {
  const fixture = await readyFixture(page);
  let uploaded = false;
  await page.route("**/api/v1/attachments", async (route) => {
    expect(route.request().headers()["x-albert-csrf"]).toBe("test-csrf");
    expect(route.request().postDataBuffer()).toEqual(picture);
    uploaded = true;
    await route.fulfill({ json: attachments[0] });
  });
  await page.goto("/ui/chats");
  await page.getByLabel("Choose attachments").setInputFiles({
    name: "Frame.png",
    mimeType: "image/png",
    buffer: picture,
  });
  await expect(
    page.getByRole("button", { name: "Remove Frame.png" }),
  ).toBeVisible();
  expect(uploaded).toBe(true);
  expect(
    fixture.requests.some((r) => r.path === "/sessions" && r.method === "POST"),
  ).toBe(false);
  expect(fixture.requests.some((r) => r.path === "/session-starts")).toBe(
    false,
  );
  await page.getByRole("button", { name: "Send", exact: true }).click();
  await expect
    .poll(
      () => fixture.requests.find((r) => r.path === "/session-starts")?.body,
    )
    .toMatchObject({ text: "", temporary: false, attachments: ["image-1"] });
});

test("ordinary documents and mixed media upload unchanged and can start an attachment-only chat", async ({
  page,
}) => {
  const fixture = await readyFixture(page);
  const files = [
    {
      name: "report.pdf",
      mimeType: "application/pdf",
      buffer: Buffer.from("%PDF-fixture"),
    },
    {
      name: "data.csv",
      mimeType: "text/csv",
      buffer: Buffer.from("name,value\na,1"),
    },
    {
      name: "archive.zip",
      mimeType: "application/zip",
      buffer: Buffer.from("PK\x03\x04fixture"),
    },
    {
      name: "opaque.bin",
      mimeType: "application/octet-stream",
      buffer: Buffer.from([0, 1, 2]),
    },
    { name: "Frame.png", mimeType: "image/png", buffer: picture },
  ];
  const uploaded: string[] = [];
  await page.route("**/api/v1/attachments", async (route) => {
    const file = files[uploaded.length];
    expect(route.request().postDataBuffer()).toEqual(file.buffer);
    expect(route.request().headers()["content-type"]).toBe(file.mimeType);
    const id = `file-${uploaded.length}`;
    uploaded.push(id);
    await route.fulfill({
      json: {
        id,
        filename: file.name,
        content_type:
          file.mimeType === "image/png"
            ? file.mimeType
            : "application/octet-stream",
        size: file.buffer.length,
      },
    });
  });
  await page.goto("/ui/chats");
  const choose = page.getByLabel("Choose attachments");
  await expect(choose).not.toHaveAttribute("accept");
  await choose.setInputFiles(files);
  for (const file of files)
    await expect(
      page.getByRole("button", { name: `Remove ${file.name}` }),
    ).toBeVisible();
  await page.getByRole("button", { name: "Send", exact: true }).click();
  await expect
    .poll(
      () =>
        fixture.requests.find((request) => request.path === "/session-starts")
          ?.body,
    )
    .toMatchObject({ text: "", attachments: uploaded });
  expect(uploaded).toHaveLength(files.length);
});

test("document history is a named download rather than an inline document preview", async ({
  page,
}) => {
  await readyFixture(page);
  await page.route("**/api/v1/sessions/*/messages?*", (route) =>
    route.fulfill({
      json: {
        items: [
          {
            ...message(1),
            content: "Please review this file",
            attachments: [
              {
                id: "document",
                filename: "report.html",
                content_type: "application/octet-stream",
                size: 2048,
              },
            ],
          },
        ],
        has_more: false,
        next_cursor: null,
        through_id: "1",
      },
    }),
  );
  await page.goto("/ui/chats/session-1");
  const download = page.locator(".message-file");
  await expect(download).toContainText("report.html");
  await expect(download).toHaveAttribute("download", "report.html");
  await expect(download).toHaveAttribute(
    "href",
    /sessions\/session-1\/attachments\/document$/,
  );
  await expect(
    page.locator(".message-body iframe, .message-body object"),
  ).toHaveCount(0);
});
