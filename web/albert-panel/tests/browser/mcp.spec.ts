import { expect, test } from "@playwright/test";
import type { components } from "../../src/api/schema";
import { connectorFixture } from "./connector-fixture";
type Config = components["schemas"]["McpServerConfig"];
test("MCP setup belongs to Connections, credentials remain references, and tool access uses revisioned native configuration", async ({
  page,
}, info) => {
  await connectorFixture(page, []);
  let config: Config | null = null,
    revision = "m1";
  const writes: { config: Config; revision: string }[] = [];
  await page.route("**/api/v1/mcp-servers**", (route) => {
    const request = route.request(),
      path = new URL(request.url()).pathname;
    if (path.endsWith("/tools"))
      return route.fulfill({
        json: {
          items: [
            {
              name: "search",
              description: "Search indexed notes",
              allowed: config?.allowed_tools.includes("search") ?? false,
            },
            { name: "write", description: "Write a note", allowed: false },
          ],
          total: 2,
          offset: 0,
          next_offset: null,
          revision,
          missing_allowed: [],
        },
      });
    if (request.method() === "POST" || request.method() === "PUT") {
      const body = request.postDataJSON();
      expect(body.revision).toBe(revision);
      writes.push(body);
      config = body.config;
      revision = revision === "m1" ? "m2" : "m3";
    }
    return route.fulfill({
      json: {
        items: config
          ? [{ config, phase: "connected", tool_count: 2, error: null }]
          : [],
        revision,
      },
    });
  });
  await page.goto("/ui/settings/connections/new");
  await page.getByRole("combobox", { name: "Connector type" }).click();
  await page.getByRole("option", { name: "MCP server", exact: true }).click();
  await page.getByRole("button", { name: "Continue", exact: true }).click();
  await expect(page).toHaveURL(/connections\/mcp\/new$/);
  await page.getByLabel("Name", { exact: true }).fill("Notes");
  await page.getByLabel("Server ID", { exact: true }).fill("notes");
  await page.getByLabel("Description", { exact: true }).fill("Search notes");
  await page
    .getByLabel("Server URL", { exact: true })
    .fill("https://example.com/mcp");
  await page
    .getByLabel("Bearer credential reference", { exact: true })
    .fill("NOTES_TOKEN");
  await page
    .getByRole("button", { name: "Connect server", exact: true })
    .click();
  await expect(page).toHaveURL(/connections\/mcp\/notes$/);
  await expect(
    page.getByRole("checkbox", { name: "Allow search", exact: true }),
  ).not.toBeChecked();
  await expect(
    page.getByRole("checkbox", { name: "Allow write", exact: true }),
  ).not.toBeChecked();
  await page
    .getByRole("checkbox", { name: "Allow search", exact: true })
    .check();
  await page.screenshot({
    path: info.outputPath("mcp-tool-access.png"),
    fullPage: true,
  });
  await page.getByRole("button", { name: "Save changes", exact: true }).click();
  await expect(page).toHaveURL(/settings\/connections$/);
  expect(writes).toHaveLength(2);
  expect(writes[0].config.connection).toEqual({
    transport: "http",
    url: "https://example.com/mcp",
    token_env: "NOTES_TOKEN",
  });
  expect(writes[1].config.allowed_tools).toEqual(["search"]);
  await expect(page.getByRole("link", { name: /Notes.*MCP/ })).toBeVisible();
});
test("stdio environment references validate inline without browser prompts or silently discarding malformed entries", async ({
  page,
}) => {
  await connectorFixture(page, []);
  await page.route("**/api/v1/mcp-servers", (route) =>
    route.fulfill({ json: { items: [], revision: "m1" } }),
  );
  await page.goto("/ui/settings/connections/mcp/new");
  await page.getByRole("combobox", { name: "Transport" }).click();
  await page
    .getByRole("option", { name: "Local stdio process", exact: true })
    .click();
  await page
    .getByLabel("Environment references", { exact: false })
    .fill("MISSING_REFERENCE");
  await expect(page.getByRole("alert")).toContainText(
    "VARIABLE=CREDENTIAL_REFERENCE",
  );
  await expect(
    page.getByRole("button", { name: "Connect server", exact: true }),
  ).toBeDisabled();
  await page
    .getByLabel("Environment references", { exact: false })
    .fill("SERVICE_TOKEN=STORED_TOKEN");
  await expect(page.getByRole("alert")).toHaveCount(0);
  await expect(
    page.getByRole("button", { name: "Connect server", exact: true }),
  ).toBeEnabled();
});
