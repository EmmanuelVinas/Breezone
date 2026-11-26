/**
 * The path the app is published under: empty at the root, `/breezone` behind a reverse proxy
 * that publishes it at `https://nas.local/breezone/`. The server gives it in the page's `<base>`;
 * the development server gives none.
 */
export const basePath = document.querySelector("base")
  ? new URL(document.baseURI).pathname.replace(/\/+$/, "")
  : ""

/** An absolute path of the server (`/api/…`) under the path of the app. */
export const appUrl = (path: string) => `${basePath}${path}`

/** The address of the server as the browser reaches it, for the phone and the MCP clients. */
export const appOrigin = () => `${window.location.origin}${basePath}`
