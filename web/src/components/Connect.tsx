import { createSignal, Show } from "solid-js";
import { pair } from "../api/client";
import { errorMessage, ApiError } from "../api/errors";
import { saveSession, type Session } from "../session/storage";
import { Icon } from "./Icon";
export function Connect(props: {
  connected: (s: Session) => void;
  expired: boolean;
}) {
  const [code, setCode] = createSignal("");
  const [busy, setBusy] = createSignal(false);
  const [error, setError] = createSignal("");
  async function submit(e: Event) {
    e.preventDefault();
    setBusy(true);
    setError("");
    try {
      const session = await pair(code().trim());
      saveSession(session);
      setCode("");
      props.connected(session);
    } catch (e) {
      setError(
        e instanceof ApiError && e.code === "unauthorized"
          ? "That code is invalid, expired, or already used. Run rhythm ui for a new one."
          : errorMessage(e),
      );
    } finally {
      setBusy(false);
    }
  }
  return (
    <main class="connect-shell">
      <div class="connect-card">
        <a class="brand" href="#/">
          <span class="brand-mark">
            <Icon name="wave" />
          </span>
          Rhythm<span class="brand-dot">.</span>
        </a>
        <div class="connect-illustration">
          <Icon name="service" />
          <span />
          <Icon name="clock" />
        </div>
        <p class="eyebrow">YOUR LOCAL WORKSPACE</p>
        <h1>Good work keeps going.</h1>
        <p class="connect-copy">
          Connect to see your services, scheduled runs, and everything that
          needs a look.
        </p>
        <div class="pair-instruction">
          <span>1</span>
          <div>
            Run this in your terminal<code>rhythm ui</code>
          </div>
        </div>
        <form onSubmit={submit}>
          <label for="pair-code">
            2 <span>Paste your single-use browser code</span>
          </label>
          <input
            id="pair-code"
            type="password"
            autocomplete="off"
            spellcheck={false}
            value={code()}
            onInput={(e) => setCode(e.currentTarget.value)}
            placeholder="Browser access code"
            required
            autofocus
          />
          <Show when={props.expired}>
            <p class="inline-notice">
              Your connection expired. Generate a new code to reconnect.
            </p>
          </Show>
          <Show when={error()}>
            <p class="form-error" role="alert">
              {error()}
            </p>
          </Show>
          <button
            class="button primary connect-button"
            type="submit"
            disabled={busy()}
          >
            {busy() ? "Connecting…" : "Connect to workspace"}
            <Icon name="arrow" />
          </button>
        </form>
        <p class="privacy-note">
          A paired tab can create and run trusted commands with your account’s
          permissions.
          <br />
          Codes expire after two minutes and work only once.
        </p>
      </div>
    </main>
  );
}
