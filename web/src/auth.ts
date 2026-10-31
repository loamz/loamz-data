// Who is signed in. The page holds one bearer token (kept in sessionStorage, so it ends with the tab) and sends it to the
// Data API. Two ways to get one: Keycloak (OIDC code flow with PKCE, a public client named `loamz-data`), or, for local
// development against a dev server, a token pasted in or passed once as `#token=…`.

const TOKEN = "loamz-data-token";
const FLOW = "loamz-data-oidc";
const authority = import.meta.env.VITE_OIDC_AUTHORITY as string | undefined;
const clientId = (import.meta.env.VITE_OIDC_CLIENT_ID as string | undefined) ?? "loamz-data";

type Listener = () => void;
const listeners = new Set<Listener>();
export function onAuthChange(fn: Listener): () => void {
  listeners.add(fn);
  return () => listeners.delete(fn);
}
const changed = () => listeners.forEach((l) => l());

/** The expiry of a JWT in milliseconds, or 0 if it cannot be read. The server decides validity; this only avoids sending a dead token. */
export function expiryOf(token: string): number {
  try {
    const part = token.split(".")[1] ?? "";
    const json = atob(part.replace(/-/g, "+").replace(/_/g, "/").padEnd(Math.ceil(part.length / 4) * 4, "="));
    const exp = (JSON.parse(json) as { exp?: unknown }).exp;
    return typeof exp === "number" ? exp * 1000 : 0;
  } catch {
    return 0;
  }
}

export function getToken(): string | null {
  try {
    const t = sessionStorage.getItem(TOKEN);
    return t && expiryOf(t) > Date.now() ? t : null;
  } catch {
    return null;
  }
}

export function setToken(token: string): void {
  try {
    sessionStorage.setItem(TOKEN, token.trim());
  } catch {
    /* storage blocked: sign-in will not survive a reload */
  }
  changed();
}

export function signOut(): void {
  try {
    sessionStorage.removeItem(TOKEN);
  } catch {
    /* nothing to remove */
  }
  changed();
}

export const hasOidc = Boolean(authority);

/** A token handed over in the address (`#token=…`) is taken once and removed from the address bar. */
export function takeTokenFromHash(): void {
  const m = /[#&]token=([^&]+)/.exec(location.hash);
  if (!m?.[1]) return;
  setToken(decodeURIComponent(m[1]));
  history.replaceState(null, "", location.pathname + location.search);
}

function random(bytes = 32): string {
  const a = crypto.getRandomValues(new Uint8Array(bytes));
  return btoa(String.fromCharCode(...a)).replace(/\+/g, "-").replace(/\//g, "_").replace(/=+$/, "");
}

async function challenge(verifier: string): Promise<string> {
  const d = await crypto.subtle.digest("SHA-256", new TextEncoder().encode(verifier));
  return btoa(String.fromCharCode(...new Uint8Array(d))).replace(/\+/g, "-").replace(/\//g, "_").replace(/=+$/, "");
}

async function endpoints(): Promise<{ authorization_endpoint: string; token_endpoint: string }> {
  const r = await fetch(`${authority}/.well-known/openid-configuration`);
  if (!r.ok) throw new Error("The sign-in service did not answer.");
  return r.json();
}

/** Sends the browser to Keycloak. */
export async function startLogin(): Promise<void> {
  if (!authority) throw new Error("Sign-in is not configured (VITE_OIDC_AUTHORITY).");
  const verifier = random();
  const state = random(16);
  sessionStorage.setItem(FLOW, JSON.stringify({ verifier, state }));
  const { authorization_endpoint } = await endpoints();
  const q = new URLSearchParams({
    client_id: clientId,
    response_type: "code",
    scope: "openid",
    redirect_uri: location.origin + "/",
    state,
    code_challenge: await challenge(verifier),
    code_challenge_method: "S256",
  });
  location.assign(`${authorization_endpoint}?${q}`);
}

/** Back from Keycloak with `?code=…`: trades it for a token. Returns false if this page load was not a sign-in return. */
export async function finishLogin(): Promise<boolean> {
  const p = new URLSearchParams(location.search);
  const code = p.get("code");
  if (!code || !authority) return false;
  const saved = JSON.parse(sessionStorage.getItem(FLOW) ?? "null") as { verifier: string; state: string } | null;
  sessionStorage.removeItem(FLOW);
  history.replaceState(null, "", location.pathname);
  if (!saved || saved.state !== p.get("state")) throw new Error("The sign-in did not match this tab. Try again.");
  const { token_endpoint } = await endpoints();
  const r = await fetch(token_endpoint, {
    method: "POST",
    headers: { "Content-Type": "application/x-www-form-urlencoded" },
    body: new URLSearchParams({ grant_type: "authorization_code", client_id: clientId, code, redirect_uri: location.origin + "/", code_verifier: saved.verifier }),
  });
  if (!r.ok) throw new Error("The sign-in service refused the code.");
  setToken(((await r.json()) as { access_token: string }).access_token);
  return true;
}
