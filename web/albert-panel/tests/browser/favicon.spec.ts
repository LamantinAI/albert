import { expect, test } from "@playwright/test";
import { clientContext } from "./fixture";

for (const entry of [
  { prefix: "", page: "models" },
  { prefix: "", page: "settings/prompts" },
  { prefix: "/tools/assistant", page: "settings/prompts" },
]) {
  test(`favicon stays reachable across route depths from ${entry.prefix}/ui/${entry.page}`, async ({
    page,
  }) => {
    const root = `${entry.prefix}/ui`;
    const response = await page.request.get("/ui/");
    const source = await response.text();
    await page.route("**/api/v1/**", (route) => {
      const path = new URL(route.request().url()).pathname
        .split("/api/v1")
        .at(-1);
      const json = (value: unknown) => route.fulfill({ json: value });
      if (path === "/context") return json(clientContext());
      if (path === "/sessions")
        return json({ items: [], has_more: false, next_cursor: null });
      if (path === "/model-pool")
        return json({ revision: "r1", preferred: null, models: [], roles: [] });
      if (path === "/connectors")
        return json({ items: [], available_types: [] });
      if (path?.startsWith("/prompts/"))
        return json({
          path: "soul",
          content: "# Persona",
          revision: "r1",
          redacted: false,
        });
      return route.fulfill({ status: 404 });
    });
    // The embedded server emits relative asset URLs for arbitrary proxy mounts.
    await page.route("**/*", async (route) => {
      const url = new URL(route.request().url());
      if (
        route.request().isNavigationRequest() &&
        url.pathname.startsWith(`${root}/`)
      ) {
        const path = url.pathname.slice(root.length + 1);
        const depth = (path.match(/\//g) ?? []).length;
        const relative = depth ? "../".repeat(depth) : "./";
        const html = source
          .replace(
            "<head>",
            `<head><meta name="albert-ui-root" content="${relative}">`,
          )
          .replace(
            /(<link\b[^>]*rel="icon"[^>]*href=")[^"]+("[^>]*>)/,
            `$1${relative}favicon.svg?v=pixel$2`,
          );
        return route.fulfill({ contentType: "text/html", body: html });
      }
      if (url.pathname === `${root}/favicon.svg`) {
        return route.fulfill({
          response: await route.fetch({
            url: new URL("/ui/favicon.svg", url).href,
          }),
        });
      }
      return route.fallback();
    });
    await page.goto(`${root}/${entry.page}`);
    const icon = page.locator('link[rel="icon"]');
    const expected = new URL(`${root}/favicon.svg?v=pixel`, page.url()).href;
    await expect(icon).toHaveAttribute("href", expected);
    const navigation = page.getByRole("navigation", {
      name: "Settings navigation",
    });
    await navigation
      .getByRole("link", {
        name: entry.page === "models" ? "Persona & instructions" : "Models",
        exact: true,
      })
      .click();
    await expect(icon).toHaveAttribute("href", expected);
    await navigation
      .getByRole("link", { name: "Connections", exact: true })
      .click();
    await expect(icon).toHaveAttribute("href", expected);
    await page.goBack();
    await expect(icon).toHaveAttribute("href", expected);
    const fetched = await icon.evaluate(async (link) => {
      const response = await fetch((link as HTMLLinkElement).href);
      return {
        status: response.status,
        type: response.headers.get("content-type"),
        body: await response.text(),
      };
    });
    expect(fetched.status).toBe(200);
    expect(fetched.type).toContain("image/svg+xml");
    expect(fetched.body).toContain("<svg");
  });
}

test("API reference declares the Albert icon beside its self-hosted assets", async ({
  page,
}) => {
  const response = await page.request.get("/ui/docs/index.html");
  const html = await response.text();
  const href = /<link\b[^>]*rel="icon"[^>]*href="([^"]+)"/.exec(html)?.[1];
  expect(href).toBeTruthy();
  const icon = await page.request.get(new URL(href!, response.url()).href);
  expect(icon.status()).toBe(200);
  expect(icon.headers()["content-type"]).toContain("image/svg+xml");
  expect(await icon.text()).toContain('fill="#caa24a"');
});
