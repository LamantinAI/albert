import type { Page } from "@playwright/test";
import type { components } from "../../src/api/schema";
import { readyFixture } from "./fixture";
type Field = components["schemas"]["ConfigField"];
const fields: Field[] = [
  {
    path: "timezone",
    group: "General",
    label: "Timezone",
    kind: "text",
    default_value: "UTC",
    description: "Timezone for dates and reminders.",
  },
  {
    path: "agent.max_tool_turns",
    group: "General",
    label: "Tool rounds per turn",
    kind: "integer",
    default_value: 8,
    minimum: 0,
    maximum: 100,
    description: "Tool budget for one request.",
  },
  {
    path: "stream_status",
    group: "General",
    label: "Live progress",
    kind: "boolean",
    default_value: true,
    description: "Show tool activity.",
  },
  {
    path: "reflection.period_secs",
    group: "Skills & reflection",
    label: "Reflection interval (seconds)",
    kind: "integer",
    default_value: 21600,
    minimum: 1,
    maximum: 9007199254740991,
    description: "Memory reflection schedule.",
  },
  {
    path: "sessions.limits.temporary_max_bytes",
    group: "Conversations",
    label: "Temporary size limit (bytes)",
    kind: "integer",
    default_value: 33554432,
    minimum: 1,
    maximum: 9007199254740991,
    description: "Storage budget for a temporary chat.",
  },
];
const escaped = (value: string) => value.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
function value(content: string, path: string) {
  const line = content.split("\n").find((line) => line.startsWith(path + "="));
  if (!line) return null;
  const text = line.slice(path.length + 1);
  try {
    return JSON.parse(text.replace(/^'(.*)'$/, '"$1"'));
  } catch {
    return text;
  }
}
function edit(content: string, path: string, item: unknown) {
  const lines = content
    .split("\n")
    .filter((line) => !line.startsWith(path + "="));
  if (item !== null) lines.push(`${path}=${JSON.stringify(item)}`);
  return lines.join("\n");
}
export async function agentFixture(page: Page) {
  await readyFixture(page);
  let content =
      '# Keep this comment\ntimezone="UTC"\nagent.max_tool_turns=8\nmemory.backend="embedded"\n',
    generation = 1;
  let config: components["schemas"]["MemorySettings"] = { backend: "embedded" };
  let phase: components["schemas"]["MemoryPhase"] = "active",
    configured = false;
  const requests: { path: string; method: string; body: any }[] = [];
  let loseSave = false,
    loseRestart = false,
    restarts = 0;
  const receipts = new Map<string, any>();
  const document = () => ({
    path: "albert",
    content,
    revision: `d${generation}`,
    redacted: false,
  });
  const memory = () => ({
    config,
    revision: `g${generation}`,
    phase,
    selection_required: false,
    model_configured: true,
    token_configured: configured,
    credential_names: [],
    error:
      phase === "initialization_failed"
        ? "Memory could not be initialized."
        : null,
  });
  await page.route("**/api/v1/configuration**", async (route) => {
    const request = route.request(),
      path = new URL(request.url()).pathname,
      method = request.method(),
      body = method === "GET" ? null : request.postDataJSON();
    requests.push({ path, method, body });
    if (path.endsWith("/configuration"))
      return route.fulfill({
        json: { items: ["albert"], revision: `g${generation}` },
      });
    if (path.endsWith("/configuration-validations"))
      return route.fulfill({
        json: { valid: true, issues: [], requires_restart: true },
      });
    if (path.endsWith("/drafts")) {
      let text = body.content;
      for (const change of body.changes)
        text = edit(text, change.path, change.value);
      return route.fulfill({
        json: {
          content: text,
          revision: body.revision,
          fields: fields.map((field) => ({
            ...field,
            value: value(text, field.path),
          })),
        },
      });
    }
    if (path.endsWith("/configuration/applications")) {
      let receipt = receipts.get(body.client_request_id);
      if (!receipt) {
        restarts++;
        receipt = {
          id: `restart-${restarts}`,
          revision: body.revision,
          state: "restarting",
          runtime_epoch: "before",
          message: "Restart requested",
        };
        receipts.set(body.client_request_id, receipt);
      }
      if (loseRestart) {
        loseRestart = false;
        return route.abort("failed");
      }
      return route.fulfill({ json: receipt });
    }
    if (path.includes("/configuration/applications/"))
      return route.fulfill({
        json: {
          id: path.split("/").at(-1),
          revision: `g${generation}`,
          state: "applied",
          runtime_epoch: "after",
          message: "Saved settings applied",
        },
      });
    if (method === "PUT") {
      if (body.revision !== document().revision)
        return route.fulfill({
          status: 409,
          json: { error: { code: "conflict", message: "Settings changed" } },
        });
      content = body.content;
      generation++;
    }
    return route.fulfill({ json: document() });
  });
  await page.route("**/api/v1/memory**", async (route) => {
    const request = route.request(),
      path = new URL(request.url()).pathname,
      method = request.method(),
      body = method === "GET" ? null : request.postDataJSON();
    requests.push({ path, method, body });
    if (path.endsWith("/initializations")) {
      phase = "active";
      return route.fulfill({ json: memory() });
    }
    if (method === "PUT") {
      if (
        body.revision !== memory().revision ||
        (body.document && body.document.revision !== document().revision)
      )
        return route.fulfill({
          status: 412,
          json: {
            error: {
              code: "revision_changed",
              message: "Settings changed; review before saving",
            },
          },
        });
      if (
        body.document &&
        value(body.document.content, "memory.backend") !==
          value(content, "memory.backend")
      )
        return route.fulfill({
          status: 409,
          json: {
            error: {
              code: "memory_document_overlap",
              message:
                "Memory was edited in both the document and Memory section",
            },
          },
        });
      content = body.document?.content ?? content;
      config = body.config;
      configured ||= !!body.token_value;
      generation++;
      content = content
        .split("\n")
        .filter((line) => !line.startsWith("memory."))
        .join("\n");
      for (const [key, item] of Object.entries(config))
        if (item != null && typeof item !== "object")
          content += `\nmemory.${key}=${JSON.stringify(item)}`;
      if (phase !== "initialization_failed") phase = "restart_required";
      if (loseSave) {
        loseSave = false;
        return route.abort("failed");
      }
      return route.fulfill({ json: { ...memory(), document: document() } });
    }
    return route.fulfill({ json: memory() });
  });
  return {
    requests,
    document,
    memory,
    loseNextSave: () => {
      loseSave = true;
    },
    loseNextRestart: () => {
      loseRestart = true;
    },
    failInitialization: () => {
      phase = "initialization_failed";
    },
    externalChange: (timezone: string) => {
      content = edit(content, "timezone", timezone);
      generation++;
    },
  };
}
