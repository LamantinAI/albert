import { test } from "@playwright/test";
import { registerSubscriptionBoundaries } from "./openai-login-subscription-boundaries";
import { registerSubscriptionRefresh } from "./openai-login-subscription-refresh";
import { registerSubscriptionSetup } from "./openai-login-subscription-setup";
test.describe.configure({ mode: "serial" });

registerSubscriptionSetup();
registerSubscriptionBoundaries();
registerSubscriptionRefresh();
