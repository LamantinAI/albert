import type { components } from "../../../api/schema";
export type SelectedSkill = components["schemas"]["ComposerSkill"];
export type ComposerPolicy = components["schemas"]["ComposerView"];
export type ComposerCommand =
  | "compact"
  | "fork"
  | "rename"
  | "temporary"
  | "restart"
  | "reload-models"
  | "settings"
  | "credentials";
