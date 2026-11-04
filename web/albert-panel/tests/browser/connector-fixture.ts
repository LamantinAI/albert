import type { Page } from "@playwright/test";
import type { components } from "../../src/api/schema";
import { readyFixture, clientContext } from "./fixture";

type Entry = components["schemas"]["ConnectorView"];
type Field = components["schemas"]["ConfigField"];
export const connectorTypes = [
  { kind: "telegram", label: "Telegram" },
  { kind: "http", label: "HTTP service" },
  { kind: "browser", label: "Browser" },
  { kind: "search", label: "Web search" },
  { kind: "storage", label: "File storage" },
];
export function field(
  path: string,
  label: string,
  value: unknown,
  kind: Field["kind"] = "text",
): Field {
  return {
    path,
    label,
    kind,
    group: "Connection",
    description: "A connector setting.",
    value,
    default_value: value,
    minimum: kind === "integer" ? 1 : null,
    maximum: null,
    required: null,
    choices: null,
  };
}
export async function connectorFixture(
  page: Page,
  entries: Entry[],
  ordinary = false,
) {
  const context = clientContext();
  if (ordinary) {
    context.account = { ...context.account!, role: "user" };
    context.caps = {
      ...context.caps,
      owner: false,
      manage_config: false,
      manage_models: false,
      manage_skills: false,
      manage_users: false,
    };
  }
  const fixture = await readyFixture(page, context);
  const items = [...entries];
  const writes: { id: string; content: string; revision: string }[] = [];
  const documents = new Map<string, { content: string; revision: string }>();
  const values = new Map<string, unknown>();
  const fields = (kind: string): Field[] =>
    kind === "telegram"
      ? [
          field(
            "connector.address_names",
            "Address names",
            ["Albert"],
            "text_list",
          ),
        ]
      : kind === "browser"
        ? [field("connector.headless", "Headless mode", true, "boolean")]
        : kind === "storage"
          ? [
              {
                ...field("connector.root", "Storage directory", null),
                required: true,
              },
            ]
          : [
              field(
                "connector.timeout_secs",
                "Timeout (seconds)",
                15,
                "integer",
              ),
            ];
  await page.route("**/api/v1/connectors", (route) =>
    route.fulfill({
      json: {
        items: ordinary
          ? items.filter((item) => item.kind === "telegram")
          : items,
        available_types: ordinary ? [] : connectorTypes,
      },
    }),
  );
  await page.route("**/api/v1/configuration/**", async (route) => {
    const request = route.request();
    const path = decodeURIComponent(new URL(request.url()).pathname).replace(
      "/api/v1/configuration/",
      "",
    );
    if (path === "applications") return route.fallback();
    const id = path.split("/")[0];
    const name = id.replace("connector:", "");
    const kind =
      (request.method() === "PUT"
        ? /type\s*=\s*["']([^"']+)["']/.exec(
            request.postDataJSON().content,
          )?.[1]
        : undefined) ??
      items.find((item) => item.id === name)?.kind ??
      new URL(page.url()).searchParams.get("type") ??
      "telegram";
    const original = documents.get(id) ?? {
      content: items.some((item) => item.id === name)
        ? `[connector]\nid = "${name}"\ntype = "${kind}"\n# retained comment\n`
        : "",
      revision: "r1",
    };
    if (path.endsWith("/drafts")) {
      const body = request.postDataJSON();
      let content = body.content;
      for (const edit of body.changes) {
        values.set(`${id}:${edit.path}`, edit.value);
        content += `\n${edit.path.replace("connector.", "")} = ${JSON.stringify(edit.value)}\n`;
      }
      return route.fulfill({
        json: {
          content,
          revision: body.revision,
          fields: fields(kind).map((f) => ({
            ...f,
            value: values.has(`${id}:${f.path}`)
              ? values.get(`${id}:${f.path}`)
              : f.value,
          })),
        },
      });
    }
    if (request.method() === "PUT") {
      const body = request.postDataJSON();
      writes.push({ id, ...body });
      documents.set(id, { content: body.content, revision: "r2" });
      if (!items.some((item) => item.id === name))
        items.push({
          id: name,
          kind,
          label:
            connectorTypes.find((type) => type.kind === kind)?.label ?? kind,
          settings_mode: "structured",
        });
    }
    return route.fulfill({
      json: { path: id, ...(documents.get(id) ?? original), redacted: false },
    });
  });
  await page.route("**/api/v1/configuration-validations", (route) =>
    route.fulfill({
      json: { valid: true, issues: [], requires_restart: true },
    }),
  );
  return { ...fixture, items, writes };
}
