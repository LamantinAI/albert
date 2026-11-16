import type { Page } from "@playwright/test";
export async function sidebarWidth(page: Page) {
  return page
    .locator(".sidebar")
    .evaluate((node) => node.getBoundingClientRect().width);
}
