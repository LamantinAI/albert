import type { ModelPool, UsageSummary } from "./usePoolResources";

export function orderedPoolModels(pool: ModelPool) {
  const preferred = pool.preferred ?? pool.models[0]?.config.id;
  const priority = new Set([
    preferred,
    ...pool.roles.flatMap((role) => role.models),
  ]);
  const seen = new Set<string>();
  return pool.models
    .filter((model) => {
      if (seen.has(model.config.id)) return false;
      seen.add(model.config.id);
      return true;
    })
    .sort(
      (a, b) =>
        Number(priority.has(b.config.id)) - Number(priority.has(a.config.id)) ||
        Number(b.config.id === preferred) - Number(a.config.id === preferred) ||
        a.priority - b.priority,
    );
}
export function modelTokens(
  summary: UsageSummary | null,
  id: string,
  model: string,
  provider?: string,
): number | null {
  const rows = summary?.models.filter(
    (row) =>
      row.model_id === id &&
      row.model === model &&
      row.total_tokens !== null &&
      (!provider ||
        row.provider === (provider === "api_key" ? "api" : provider)),
  );
  return rows?.length
    ? rows.reduce((total, row) => total + (row.total_tokens ?? 0), 0)
    : null;
}
export function tokenLabel(value: number | null) {
  return value === null
    ? "—"
    : new Intl.NumberFormat("en", {
        notation: "compact",
        maximumFractionDigits: 1,
      }).format(value);
}
export function providerLabel(provider: string, base?: string | null) {
  if (provider === "subscription") return "OpenAI subscription";
  try {
    return base ? new URL(base).hostname : "API";
  } catch {
    return "API";
  }
}
export function windowLabel(seconds: number) {
  if (seconds === 604800) return "Weekly window";
  if (seconds % 86400 === 0) return `${seconds / 86400}-day window`;
  if (seconds % 3600 === 0) return `${seconds / 3600}-hour window`;
  return `${Math.ceil(seconds / 60)}-minute window`;
}
export function resetLabel(timestamp: number) {
  const minutes = Math.max(
    0,
    Math.ceil((timestamp * 1000 - Date.now()) / 60000),
  );
  if (!minutes) return "Reset due";
  return minutes >= 1440
    ? `Resets in ${Math.floor(minutes / 1440)}d ${Math.floor((minutes % 1440) / 60)}h`
    : `Resets in ${Math.floor(minutes / 60)}h ${minutes % 60}m`;
}
