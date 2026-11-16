import { describe, expect, it } from "vitest";
import { modelTokens, orderedPoolModels, tokenLabel } from "./poolPresentation";
import type { ModelPool, UsageSummary } from "./usePoolResources";
const member = (id: string, priority: number) => ({
  priority,
  credential_configured: true,
  config: {
    id,
    model: id,
    provider: "api_key",
    base_url: null,
    api_key_env: null,
    context_window: null,
    vision: false,
    tools: true,
    reasoning: null,
    request_timeout_ms: null,
  },
});
describe("model usage presentation", () => {
  it("prioritizes operator and configured role models without duplicating or hiding the rest", () => {
    const pool: ModelPool = {
      revision: "one",
      preferred: "operator",
      models: [
        member("unused", 0),
        member("research", 1),
        member("operator", 2),
      ],
      roles: [
        {
          id: "first",
          description: "",
          models: ["research"],
          reasoning_effort: null,
        },
        {
          id: "second",
          description: "",
          models: ["research"],
          reasoning_effort: null,
        },
      ],
    };
    expect(orderedPoolModels(pool).map((m) => m.config.id)).toEqual([
      "operator",
      "research",
      "unused",
    ]);
    expect(
      orderedPoolModels({ ...pool, roles: [] }).map((m) => m.config.id),
    ).toEqual(["operator", "unused", "research"]);
  });
  it("keeps unreported usage unknown and accepts explicit zero without including an older model snapshot", () => {
    const summary: UsageSummary = {
      tracking_started_at: "2030-01-01T00:00:00Z",
      period_started_at: "2030-01-01T00:00:00Z",
      pending_records: 0,
      lost_records: 0,
      models: [
        {
          model_id: "operator",
          model: "current",
          provider: "api",
          input_tokens: null,
          output_tokens: null,
          cached_input_tokens: null,
          total_tokens: null,
          reported_calls: 0,
          unreported_calls: 1,
        },
      ],
    };
    expect(tokenLabel(modelTokens(summary, "operator", "current"))).toBe("—");
    summary.models[0].total_tokens = 0;
    expect(modelTokens(summary, "operator", "current")).toBe(0);
    expect(modelTokens(summary, "operator", "renamed")).toBeNull();
    expect(
      modelTokens(summary, "operator", "current", "subscription"),
    ).toBeNull();
    expect(modelTokens(summary, "operator", "current", "api_key")).toBe(0);
  });
});
