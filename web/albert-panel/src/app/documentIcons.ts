/** History navigation changes the base of relative icon URLs in some browsers. */
export function stabilizeDocumentIcons(page: Document = document) {
  for (const link of page.querySelectorAll<HTMLLinkElement>(
    'link[rel~="icon"], link[rel="apple-touch-icon"]',
  )) {
    const href = link.getAttribute("href");
    if (!href) continue;
    try {
      link.href = new URL(href, page.baseURI).href;
    } catch {
      // An invalid optional icon must not prevent the workspace from opening.
    }
  }
}
