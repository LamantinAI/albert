import { expect, test } from "@playwright/test";
import { connectorFixture } from "./connector-fixture";
import { clientContext } from "./fixture";

test("all settings returns share the same flat link appearance, including hover", async ({
  page,
}, testInfo) => {
  await connectorFixture(page, [
    {
      id: "telegram",
      kind: "telegram",
      label: "Telegram",
      settings_mode: "structured",
    },
  ]);
  await page.route("**/api/v1/skills", (route) =>
    route.fulfill({ json: { items: [] } }),
  );
  await page.route("**/api/v1/users", (route) =>
    route.fulfill({ json: { items: [clientContext().account] } }),
  );
  const appearance = async (name: string) => {
    const back = page.locator(".settings-back");
    await expect(back).toHaveCount(1);
    await back.hover();
    await expect(back).toHaveCSS("background-color", "rgba(0, 0, 0, 0)");
    await expect(back).toHaveCSS("border-top-width", "0px");
    await back.screenshot({ path: testInfo.outputPath(`${name}.png`) });
    return back.evaluate((node) => {
      const style = getComputedStyle(node);
      return {
        font: style.fontFamily,
        size: style.fontSize,
        weight: style.fontWeight,
        line: style.lineHeight,
        color: style.color,
        padding: style.padding,
        radius: style.borderRadius,
        height: node.getBoundingClientRect().height,
        arrow: node.querySelector("svg")?.getAttribute("width"),
      };
    });
  };
  await page.goto("/ui/settings/connections/new");
  const baseline = await appearance("connections-back");
  await page.goto("/ui/models");
  await page.getByRole("button", { name: "Add model", exact: true }).click();
  await page.getByRole("button", { name: "API", exact: true }).click();
  expect(await appearance("models-back")).toEqual(baseline);
  await page.goto("/ui/skills");
  await page.getByRole("button", { name: "Add skill", exact: true }).click();
  await page.getByRole("button", { name: "Create skill", exact: true }).click();
  expect(await appearance("skills-back")).toEqual(baseline);
  await page.goto("/ui/users");
  await page.getByRole("button", { name: "Add person", exact: true }).click();
  expect(await appearance("people-back")).toEqual(baseline);
});
