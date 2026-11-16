import { test } from "@playwright/test";
import { registerFirstLaunch } from "./backend-first-launch";
import { registerOwnerControls } from "./backend-owner-controls";
import { registerReferenceAccess } from "./backend-reference-access";
import { registerStructuredEditing } from "./backend-structured-editing";
test.describe.configure({ mode: "serial" });
test.skip(
  !process.env.ALBERT_E2E_URL || !process.env.ALBERT_E2E_MODEL_ORIGIN,
  "Set ALBERT_E2E_URL and ALBERT_E2E_MODEL_ORIGIN to an isolated fresh instance and local mock provider.",
);

registerFirstLaunch();
registerOwnerControls();
registerReferenceAccess();
registerStructuredEditing();
