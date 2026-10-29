import { defineConfig } from "@playwright/test";

export default defineConfig({
  testDir: "./tests",
  timeout: 30_000,
  expect: { timeout: 7_000 },
  fullyParallel: false,
  workers: 1,
  use: {
    baseURL: "http://127.0.0.1:5187",
    viewport: { width: 1440, height: 1000 },
    reducedMotion: "reduce",
    screenshot: "only-on-failure",
    trace: "retain-on-failure",
    launchOptions: {
      ...(process.env.ALBERT_BROWSER_PATH
        ? { executablePath: process.env.ALBERT_BROWSER_PATH }
        : {}),
      args: ["--no-sandbox"],
    },
  },
  webServer: process.env.ALBERT_E2E_URL
    ? undefined
    : {
        command:
          "npm run build && npm run preview -- --host 127.0.0.1 --port 5187 --strictPort",
        url: "http://127.0.0.1:5187/ui/",
        reuseExistingServer: false,
      },
  projects: [
    { name: "browser-components", testMatch: "**/browser/*.spec.ts" },
    {
      name: "real-backend",
      testMatch: "**/integration/*.spec.ts",
      use: { baseURL: process.env.ALBERT_E2E_URL },
    },
  ],
});
