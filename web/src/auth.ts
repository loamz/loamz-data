// Who is signed in. People sign in through Keycloak (authorization code flow with PKCE; the public client `loamz-data`).
// The page keeps the session in sessionStorage, so it ends with the tab, and renews the access token with the refresh
// token shortly before it expires. There is no other way to sign in.

const SESSION = "loamz-data-session";
const FLOW = "loamz-data-oidc";
const authority = import.meta.env.VITE_OIDC_AUTHORITY as string | undefined;
const clientId = (import.meta.env.VITE_OIDC_CLIENT_ID as string | undefined) ?? "loamz-data";

interface Session {
  access: string;
  refresh?: string;
  /** When the access token stops working, in milliseconds. */
  expires: number;
  /** For signing out of Keycloak too. */
  id?: string;
}

type Listener = () => void;
const listeners = new Set<Listener>();
export function onAuthChange(fn: Listener): () => void {
  listeners.add(fn);
  return () => listeners.delete(fn);
}
const changed = () => listeners.forEach((l) => l());

function read(): Session | null {
  try {
    return JSON.parse(sessionStorage.getItem(SESSION) ?? "null") as Session | null;
  } catch {
    return null;
  }
}

function write(s: Session | null): void {
  try {
    if (s) sessionStorage.setItem(SESSION, JSON.stringify(s));
    else sessionStorage.removeItem(SESSION);
  } catch {
    /* storage blocked: the session lasts until the page closes */
  }
  memory = s;
  changed();
}

let memory: Session | null = read();

/** Whether there is a session that can still be used or renewed. */
export function signedIn(): boolean {
  const s = memory ?? read();
  return Boolean(s && (s.expires > Date.now() || s.refresh));
}

/** The expiry of a JWT in milliseconds, or 0 if it cannot be read. */
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

let renewing: Promise<string | null> | null = null;

/** A token that is good for at least the next half minute, renewed if need be; null when the person must sign in again. */
export async function token(): Promise<string | null> {
  const s = memory ?? read();
  if (!s) return null;
  if (s.expires - 30_000 > Date.now()) return s.access;
  if (!s.refresh) {
    write(null);
    return null;
  }
  renewing ??= renew(s).finally(() => (renewing = null));
  return renewing;
}

async function renew(s: Session): Promise<string | null> {
  try {
    const { token_endpoint } = await endpoints();
    const r = await fetch(token_endpoint, {
      method: "POST",
      headers: { "Content-Type": "application/x-www-form-urlencoded" },
      body: new URLSearchParams({ grant_type: "refresh_token", client_id: clientId, refresh_token: s.refresh ?? "" }),
    });
    if (!r.ok) throw new Error("refused");
    return keep((await r.json()) as TokenAnswer, s.id);
  } catch {
    write(null);
    return null;
  }
}

interface TokenAnswer {
  access_token: string;
  refresh_token?: string;
  id_token?: string;
  expires_in?: number;
}

function keep(a: TokenAnswer, previousId?: string): string {
  const expires = a.expires_in ? Date.now() + a.expires_in * 1000 : expiryOf(a.access_token);
  write({ access: a.access_token, refresh: a.refresh_token, expires, id: a.id_token ?? previousId });
  return a.access_token;
}

export const hasOidc = Boolean(authority);

function random(bytes = 32): string {
  const a = crypto.getRandomValues(new Uint8Array(bytes));
  return btoa(String.fromCharCode(...a)).replace(/\+/g, "-").replace(/\//g, "_").replace(/=+$/, "");
}

async function challenge(verifier: string): Promise<string> {
  const d = await crypto.subtle.digest("SHA-256", new TextEncoder().encode(verifier));
  return btoa(String.fromCharCode(...new Uint8Array(d))).replace(/\+/g, "-").replace(/\//g, "_").replace(/=+$/, "");
}

let discovered: { authorization_endpoint: string; token_endpoint: string; end_session_endpoint?: string } | null = null;
async function endpoints() {
  if (discovered) return discovered;
  const r = await fetch(`${authority}/.well-known/openid-configuration`);
  if (!r.ok) throw new Error("The sign-in service did not answer.");
  discovered = await r.json();
  return discovered!;
}

/** Sends the browser to Keycloak, and brings it back to where it was. */
export async function startLogin(): Promise<void> {
  if (!authority) throw new Error("Sign-in is not set up for this address (VITE_OIDC_AUTHORITY).");
  const verifier = random();
  const state = random(16);
  sessionStorage.setItem(FLOW, JSON.stringify({ verifier, state, back: location.pathname + location.search }));
  const { authorization_endpoint } = await endpoints();
  const q = new URLSearchParams({
    client_id: clientId,
    response_type: "code",
    // `organization:*` asks for every organization the person belongs to, not just one.
    scope: "openid organization:*",
    redirect_uri: location.origin + "/",
    state,
    code_challenge: await challenge(verifier),
    code_challenge_method: "S256",
  });
  location.assign(`${authorization_endpoint}?${q}`);
}

/** Back from Keycloak with `?code=…`: trades it for a session. Returns false if this page load was not a sign-in return. */
export async function finishLogin(): Promise<boolean> {
  const p = new URLSearchParams(location.search);
  const code = p.get("code");
  if (!code || !authority) return false;
  const saved = JSON.parse(sessionStorage.getItem(FLOW) ?? "null") as { verifier: string; state: string; back: string } | null;
  sessionStorage.removeItem(FLOW);
  if (!saved || saved.state !== p.get("state")) {
    history.replaceState(null, "", "/");
    throw new Error("The sign-in did not match this tab. Try again.");
  }
  const { token_endpoint } = await endpoints();
  const r = await fetch(token_endpoint, {
    method: "POST",
    headers: { "Content-Type": "application/x-www-form-urlencoded" },
    body: new URLSearchParams({ grant_type: "authorization_code", client_id: clientId, code, redirect_uri: location.origin + "/", code_verifier: saved.verifier }),
  });
  history.replaceState(null, "", saved.back && saved.back !== "/" ? saved.back : "/");
  if (!r.ok) throw new Error("The sign-in service refused the code.");
  keep((await r.json()) as TokenAnswer);
  return true;
}

/** Ends the session here and in Keycloak. */
export async function signOut(): Promise<void> {
  const s = memory ?? read();
  write(null);
  try {
    const { end_session_endpoint } = await endpoints();
    if (end_session_endpoint) {
      const q = new URLSearchParams({ client_id: clientId, post_logout_redirect_uri: location.origin + "/" });
      if (s?.id) q.set("id_token_hint", s.id);
      location.assign(`${end_session_endpoint}?${q}`);
    }
  } catch {
    /* Keycloak did not answer: signed out here at least */
  }
}
