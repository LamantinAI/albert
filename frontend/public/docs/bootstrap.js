/* Self-hosted Swagger UI. Authentication stays in the HttpOnly browser cookie. */
async function openReference() {
  const response = await fetch("/api/v1/openapi.json", {
    credentials: "same-origin",
  });
  if (!response.ok) throw new Error("The OpenAPI schema could not be loaded.");
  const spec = await response.json();
  // Swagger validates required inputs before requestInterceptor. Keep the
  // required header visible and supply a display default; every mutation below
  // replaces it with the current browser token. The downloadable schema stays exact.
  for (const path of Object.values(spec.paths)) {
    for (const operation of Object.values(path)) {
      for (const parameter of operation.parameters || []) {
        if (parameter.in === "header" && parameter.name === "x-albert-csrf") {
          parameter.schema = {
            ...parameter.schema,
            default: "supplied-by-browser-session",
          };
          parameter.description =
            "Automatically supplied from your current browser session.";
        }
      }
    }
  }
  window.albertApiDocs = SwaggerUIBundle({
    spec,
    dom_id: "#swagger-ui",
    deepLinking: true,
    validatorUrl: null,
    queryConfigEnabled: false,
    persistAuthorization: false,
    withCredentials: true,
    displayRequestDuration: true,
    requestInterceptor: async (request) => {
      const url = new URL(request.url, window.location.origin);
      if (
        url.origin !== window.location.origin ||
        !url.pathname.startsWith("/api/v1/")
      ) {
        throw new Error("API requests must use this Albert workspace.");
      }
      request.credentials = "same-origin";
      if (
        !["GET", "HEAD", "OPTIONS"].includes(
          (request.method || "GET").toUpperCase(),
        ) &&
        url.pathname !== "/api/v1/auth/credentials"
      ) {
        const context = await fetch("/api/v1/context", {
          credentials: "same-origin",
          cache: "no-store",
        }).then((response) => response.json());
        if (context.csrf)
          request.headers = {
            ...request.headers,
            "x-albert-csrf": context.csrf,
          };
      }
      return request;
    },
  });
}
openReference().catch((error) => {
  document.querySelector("#swagger-ui").textContent = error.message;
});
