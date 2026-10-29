import { api } from "../../api/client";
import type { components } from "../../api/schema";
import type { ModelDraft } from "./ModelForm";

export type ModelPool = components["schemas"]["ModelPoolView"];
export type Model = components["schemas"]["ModelView"];

export async function saveModel(
  draft: ModelDraft,
  pool?: ModelPool,
  existing?: Model,
) {
  const config: components["schemas"]["ModelConfig"] = {
    ...(existing?.config ?? {}),
    id: draft.id.trim(),
    model: draft.model.trim(),
    provider: existing?.config.provider ?? "api_key",
    base_url: draft.baseUrl.trim() || null,
    api_key_env: draft.secretName.trim() || null,
    vision: draft.vision,
    tools: draft.tools,
    context_window: draft.contextWindow ? Number(draft.contextWindow) : null,
  };
  const body = {
    config,
    api_key: draft.secretValue || null,
    revision: pool?.revision ?? null,
  };
  return existing
    ? api("/models/{id}", "put", { path: { id: existing.config.id }, body })
    : api("/models", "post", { body });
}
