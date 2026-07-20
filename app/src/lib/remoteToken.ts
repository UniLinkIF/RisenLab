// Remote-access token plumbing (see app/src-tauri/src/remote.rs for the server side). A
// colleague opens `https://<tunnel>.trycloudflare.com/?token=<token>` — this reads that token
// ONCE off the URL on first load, stashes it in sessionStorage (survives client-side navigation,
// gone when the tab closes), and strips it from the visible address bar so it doesn't linger in
// browser history/screenshots. `api.ts` attaches it to every `/api/*` call from then on via the
// `X-RisenLab-Token` header — the same header name the Rust server checks.
const STORAGE_KEY = "risenlab_remote_token";

/** Call once at app startup (see `main.tsx`). No-op (and harmless) for the local Tauri app and
 * the `npm run dev` browser preview — neither URL ever carries a `?token=`. */
export function initRemoteToken(): void {
  if (typeof window === "undefined") return;
  const params = new URLSearchParams(window.location.search);
  const token = params.get("token");
  if (!token) return;
  window.sessionStorage.setItem(STORAGE_KEY, token);
  // Cosmetic + a little privacy: don't leave the raw token sitting in the visible URL/history
  // after it's been captured.
  params.delete("token");
  const rest = params.toString();
  const clean = window.location.pathname + (rest ? `?${rest}` : "") + window.location.hash;
  window.history.replaceState(null, "", clean);
}

export function getRemoteToken(): string | null {
  if (typeof window === "undefined") return null;
  return window.sessionStorage.getItem(STORAGE_KEY);
}
