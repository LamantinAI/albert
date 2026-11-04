/** Resolve once: SPA navigation must not change the document's deployment root. */
export function resolveUiRoot(relative: string, documentUrl: string) {
  const document = new URL(documentUrl);
  const root = new URL(relative, document);
  if (root.origin !== document.origin || root.pathname.startsWith("//"))
    throw new Error("Invalid workspace path.");
  return root.pathname.replace(/\/+$/, "");
}

const relative = document.querySelector<HTMLMetaElement>(
  'meta[name="albert-ui-root"]',
)?.content;
const embeddedRoot =
  relative === undefined ? null : resolveUiRoot(relative, window.location.href);

export function apiPath(path: string) {
  return `${embeddedRoot ?? ""}/api/v1${path}`;
}

export function uiPath(path = "") {
  return `${embeddedRoot ?? "/ui"}${path}`;
}
