import { sessionSchema } from "../api/contracts";
export const sessionKey = "rhythm.browser-session";
export type Session = { token: string; demo: boolean };
export function readSession(): Session | undefined {
  try {
    const raw = sessionStorage.getItem(sessionKey);
    const parsed = raw ? sessionSchema.safeParse(JSON.parse(raw)) : undefined;
    return parsed?.success ? parsed.data : undefined;
  } catch {
    return undefined;
  }
}
export function saveSession(session: Session) {
  try {
    sessionStorage.setItem(sessionKey, JSON.stringify(session));
  } catch {
    /* The current connection can still live in memory. */
  }
}
export function clearSession() {
  try {
    sessionStorage.removeItem(sessionKey);
  } catch {
    /* In-memory credentials are cleared by the caller. */
  }
}
