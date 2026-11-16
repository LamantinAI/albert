export const sentinel = "sensitive-fixture-value-49";
export async function browserStorage(page: import("@playwright/test").Page) {
  return page.evaluate(() =>
    [...Object.values(sessionStorage), ...Object.values(localStorage)].join(
      " ",
    ),
  );
}
