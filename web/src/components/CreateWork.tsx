import { createSignal, Show } from "solid-js";
import { createdSchema, scheduleCreatedSchema } from "../api/contracts";
import type { Api } from "../api/client";
import { errorMessage } from "../api/errors";
import { parseArguments, type Kind } from "../workloads/presentation";
import { Dialog } from "./Dialog";
import { Icon } from "./Icon";
export function CreateWork(props: {
  kind: Kind;
  api: Api;
  unavailable: boolean;
  close: () => void;
  created: (id: string) => void;
}) {
  const [cadence, setCadence] = createSignal("interval");
  const [busy, setBusy] = createSignal(false);
  const [error, setError] = createSignal("");
  async function submit(e: SubmitEvent) {
    e.preventDefault();
    if (props.unavailable || busy()) return;
    setBusy(true);
    setError("");
    const data = new FormData(e.currentTarget as HTMLFormElement);
    const text = (key: string) => String(data.get(key) ?? "");
    const definition: Record<string, unknown> = {
      id: text("id"),
      command: [text("program"), ...parseArguments(text("arguments"))],
      working_directory: text("directory"),
    };
    if (props.kind === "service") {
      definition.restart = text("restart");
      definition.restart_delay_ms = Number(text("restart_delay"));
    } else {
      definition.trigger =
        cadence() === "interval"
          ? { type: "interval", every_ms: Number(text("interval")) * 1000 }
          : {
              type: cadence(),
              hour: Number(text("time").split(":")[0]),
              minute: Number(text("time").split(":")[1]),
              timezone: text("timezone"),
              ...(cadence() === "weekly"
                ? { weekdays: data.getAll("weekday").map(Number) }
                : {}),
            };
      definition.overlap = text("overlap");
      definition.missed = text("missed");
      definition.grace_ms = 1000;
      if (cadence() === "weekly" && data.getAll("weekday").length === 0) {
        setError("Choose at least one weekday.");
        setBusy(false);
        return;
      }
    }
    try {
      const request =
        props.kind === "service"
          ? { operation: "create", definition }
          : {
              operation: "schedule",
              command: { operation: "create", definition },
            };
      if (props.kind === "service") await props.api(request, createdSchema);
      else await props.api(request, scheduleCreatedSchema);
      props.created(text("id"));
    } catch (e) {
      setError(errorMessage(e));
    } finally {
      setBusy(false);
    }
  }
  return (
    <Dialog
      title={`New ${props.kind}`}
      close={() => {
        if (!busy()) props.close();
      }}
      class="create-dialog"
    >
      <p class="eyebrow">NEW {props.kind.toUpperCase()}</p>
      <h2>Give work a place to live.</h2>
      <p class="dialog-description">
        {props.kind === "service"
          ? "Create a stopped service. Start it when you’re ready."
          : "Create a paused schedule. Try a run before enabling its trigger."}
      </p>
      <Show when={props.unavailable}>
        <p class="form-error" role="alert">
          Connection lost. Reconnect before creating work.
        </p>
      </Show>
      <form onSubmit={submit}>
        <label for="work-id">
          Name <small>Lowercase letters, numbers, hyphens, underscores</small>
        </label>
        <input
          id="work-id"
          name="id"
          required
          pattern="[a-z][a-z0-9_-]{0,62}"
          placeholder="my-worker"
          autofocus
        />
        <div class="field-heading">
          <Icon name="terminal" />
          <span>Execution</span>
          <span class="subtle">Trusted foreground commands</span>
        </div>
        <label for="program">Executable</label>
        <input
          id="program"
          name="program"
          required
          pattern="/.*"
          value="/usr/bin/true"
          spellcheck={false}
        />
        <label for="arguments">
          Arguments <small>One argument per line, passed literally</small>
        </label>
        <textarea
          id="arguments"
          name="arguments"
          rows="3"
          placeholder="Optional arguments"
          spellcheck={false}
        />
        <label for="directory">Working directory</label>
        <input
          id="directory"
          name="directory"
          required
          pattern="/.*"
          value="/tmp"
          spellcheck={false}
        />
        <Show when={props.kind === "service"}>
          <div class="field-grid">
            <div>
              <label for="restart-policy">Restart policy</label>
              <select id="restart-policy" name="restart">
                <option value="never">Never restart</option>
                <option value="on_failure">On failure</option>
                <option value="always">Always</option>
              </select>
            </div>
            <div>
              <label for="restart-delay">Restart delay (ms)</label>
              <input
                id="restart-delay"
                name="restart_delay"
                type="number"
                min="100"
                max="60000"
                value="1000"
                required
              />
            </div>
          </div>
        </Show>
        <Show when={props.kind === "schedule"}>
          <div class="field-heading">
            <Icon name="clock" />
            <span>Timing & policies</span>
          </div>
          <label for="trigger-type">Trigger</label>
          <select
            id="trigger-type"
            onChange={(e) => setCadence(e.currentTarget.value)}
          >
            <option value="interval">Interval</option>
            <option value="daily">Daily</option>
            <option value="weekly">Weekly</option>
          </select>
          <Show
            when={cadence() === "interval"}
            fallback={
              <div class="field-grid">
                <div>
                  <label for="trigger-time">Time</label>
                  <input
                    id="trigger-time"
                    name="time"
                    type="time"
                    value="09:00"
                    required
                  />
                </div>
                <div>
                  <label for="timezone">IANA timezone</label>
                  <input
                    id="timezone"
                    name="timezone"
                    value="UTC"
                    required
                    spellcheck={false}
                  />
                </div>
              </div>
            }
          >
            <label for="interval">Every (seconds)</label>
            <input
              id="interval"
              name="interval"
              type="number"
              min="0.1"
              max="31536000"
              step="0.1"
              value="60"
              required
            />
          </Show>
          <Show when={cadence() === "weekly"}>
            <fieldset class="weekdays">
              <legend>Weekdays</legend>
              {["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"].map(
                (day, i) => (
                  <label>
                    <input
                      type="checkbox"
                      name="weekday"
                      value={i + 1}
                      checked={i === 0}
                    />
                    <span>{day}</span>
                  </label>
                ),
              )}
            </fieldset>
          </Show>
          <div class="field-grid">
            <div>
              <label for="overlap">When already running</label>
              <select id="overlap" name="overlap">
                <option value="skip">Skip the next run</option>
                <option value="queue_one">Queue one latest run</option>
              </select>
            </div>
            <div>
              <label for="missed">After missed runs</label>
              <select id="missed" name="missed">
                <option value="skip">Skip expired runs</option>
                <option value="run_once">Catch up once</option>
              </select>
            </div>
          </div>
          <p class="field-note">
            Calendar times skip DST gaps and use the earlier repeated time.
            Missed runs have a one-second grace window.
          </p>
        </Show>
        <Show when={error()}>
          <p class="form-error" role="alert">
            {error()}
          </p>
        </Show>
        <div class="dialog-footer">
          <button
            class="button"
            type="button"
            onClick={props.close}
            disabled={busy()}
          >
            Cancel
          </button>
          <button
            class="button primary"
            type="submit"
            disabled={busy() || props.unavailable}
          >
            {busy() ? "Creating…" : `Create ${props.kind}`}
            <Icon name="plus" />
          </button>
        </div>
      </form>
    </Dialog>
  );
}
