import {
  createResource,
  createSignal,
  onCleanup,
  onMount,
  Show,
  For,
  createEffect,
  createUniqueId,
} from "solid-js";
import { createStore, reconcile } from "solid-js/store";
import type { Api } from "../api/client";
import { errorMessage } from "../api/errors";
import {
  logsSchema,
  serviceDetailSchema,
  scheduleDetailSchema,
  type Execution,
} from "../api/contracts";
import { loadDetail, type Detail } from "../workloads/detail";
import {
  status,
  outcome,
  when,
  timestamp,
  cadence,
  isUncertain,
  type Kind,
} from "../workloads/presentation";
import { Dialog } from "./Dialog";
import { Icon, type IconName } from "./Icon";

type Action = {
  operation: string;
  title: string;
  description: string;
  label: string;
  danger?: boolean;
  ack?: number;
};
export function WorkDetail(props: {
  kind: Kind;
  id: string;
  api: Api;
  offline: boolean;
  close: () => void;
  changed: (message: string) => void;
}) {
  const [loaded, { refetch }] = createResource(
    () => ({ kind: props.kind, id: props.id }),
    (key) => loadDetail(props.api, key.kind, key.id),
  );
  const [state, setState] = createStore<{ detail?: Detail }>({});
  const detail = () => state.detail;
  createEffect(() => {
    const id = props.id;
    const kind = props.kind;
    if (loaded.error) return;
    const value = loaded();
    if (value?.definition.id === id && value.kind === kind)
      setState("detail", reconcile(value, { key: "id" }));
  });
  const [tab, setTab] = createSignal("activity");
  const tabId = createUniqueId();
  const [busy, setBusy] = createSignal(false);
  const [error, setError] = createSignal("");
  const [confirm, setConfirm] = createSignal<Action>();
  const [cleaned, setCleaned] = createSignal(false);
  const [selectedRun, setSelectedRun] = createSignal<Execution>();
  const [logs, setLogs] = createSignal<{ stdout: string; stderr: string }>();
  const [logsError, setLogsError] = createSignal("");
  const [logsBusy, setLogsBusy] = createSignal(false);
  let disposed = false;
  let logRequest = 0;
  onMount(() => {
    const timer = setInterval(() => {
      if (!busy()) void refreshDetail();
    }, 2000);
    onCleanup(() => {
      clearInterval(timer);
      disposed = true;
      logRequest++;
    });
  });
  createEffect(() => {
    props.id;
    props.kind;
    setState("detail", undefined);
    setError("");
    setConfirm(undefined);
    setLogs(undefined);
    setSelectedRun(undefined);
    setTab("activity");
    logRequest++;
  });
  async function refreshDetail() {
    try {
      await refetch();
    } catch {
      /* The resource exposes a retryable error state. */
    }
  }
  async function perform(action: Action) {
    const targetId = props.id;
    const targetKind = props.kind;
    setBusy(true);
    setError("");
    try {
      const request = {
        operation: action.operation,
        id: targetId,
        ...(action.ack ? { execution_id: action.ack } : {}),
      };
      if (targetKind === "service")
        await props.api(request, serviceDetailSchema);
      else
        await props.api(
          { operation: "schedule", command: request },
          scheduleDetailSchema,
        );
      if (disposed || props.id !== targetId || props.kind !== targetKind)
        return;
      setConfirm(undefined);
      setCleaned(false);
      props.changed(`${action.label} requested · ${targetId}`);
      await refreshDetail();
    } catch (e) {
      if (!disposed && props.id === targetId && props.kind === targetKind)
        setError(errorMessage(e));
    } finally {
      if (!disposed) setBusy(false);
    }
  }
  function ask(action: Action) {
    setError("");
    setCleaned(false);
    setConfirm(action);
  }
  async function showLogs(run: Execution) {
    const request = ++logRequest;
    setSelectedRun(run);
    setLogs(undefined);
    setLogsError("");
    setLogsBusy(true);
    try {
      const response = await props.api(
        { operation: "logs", execution_id: run.id },
        logsSchema,
      );
      if (request === logRequest && !disposed) setLogs(response);
    } catch (e) {
      if (request === logRequest && !disposed) setLogsError(errorMessage(e));
    } finally {
      if (request === logRequest && !disposed) setLogsBusy(false);
    }
  }
  const unavailable = () => props.offline || busy() || !!loaded.error;
  const blocked = () =>
    ["needs_attention", "recovering"].includes(
      detail()?.value.observed.state ?? "",
    );
  function button(
    label: string,
    icon: IconName,
    action: Action,
    confirmation = false,
    disabled = false,
    primary = false,
  ) {
    return (
      <button
        class={`button ${primary ? "primary" : ""}`}
        disabled={unavailable() || disabled}
        onClick={() => (confirmation ? ask(action) : void perform(action))}
      >
        <Icon name={icon} />
        {label}
      </button>
    );
  }
  return (
    <>
      <Dialog
        title={props.id}
        close={() => {
          if (!busy()) props.close();
        }}
        class="detail-dialog"
      >
        <p class="eyebrow">RHYTHM-OWNED {props.kind.toUpperCase()}</p>
        <h2>{props.id}</h2>
        <Show
          when={detail()}
          fallback={
            <div class="detail-loading">
              {loaded.error ? (
                <div role="alert">
                  <h3>Couldn’t open this workload</h3>
                  <p>{errorMessage(loaded.error)}</p>
                  <button class="button" onClick={() => void refreshDetail()}>
                    Retry
                  </button>
                </div>
              ) : (
                <p class="loading-line">Loading activity…</p>
              )}
            </div>
          }
        >
          {(data) => {
            const item = () => ({ kind: data().kind, value: data().value });
            const state = () => status(item());
            const enabled = () => {
              const v = data().value;
              return "enabled" in v && v.enabled;
            };
            const hasPending = () => {
              const v = data().value;
              return "pending" in v && v.pending !== null;
            };
            const desiredLabel = () => {
              const v = data().value;
              return "desired" in v
                ? v.desired
                : v.enabled
                  ? "Enabled"
                  : "Paused";
            };
            const nextLabel = () => {
              const v = data().value;
              return "next_run_at_ms" in v
                ? when(v.next_run_at_ms)
                : outcome(v.latest_execution);
            };
            return (
              <>
                <div class="detail-status">
                  <span class={`status ${state().tone}`}>
                    <i />
                    {state().label}
                  </span>
                  <span class="subtle">
                    {data().kind === "service"
                      ? "Execution state · application health not reported"
                      : "Finite executions · explicit trigger policies"}
                  </span>
                </div>
                <Show when={isUncertain(data().value.latest_execution)}>
                  <div class="attention-message">
                    <Icon name="alert" />
                    <div>
                      <strong>Completion is uncertain</strong>
                      <p>
                        New work is blocked. Inspect and manually clean up
                        execution #{data().value.latest_execution!.id} before
                        acknowledging it.
                      </p>
                      <button
                        class="text-button"
                        disabled={unavailable()}
                        onClick={() =>
                          ask({
                            operation: "acknowledge",
                            title: "Acknowledge cleanup?",
                            description:
                              "This records your confirmation, preserves the original outcome, and leaves the workload stopped or paused. It does not terminate unknown processes.",
                            label: "Acknowledge cleanup",
                            ack: data().value.latest_execution!.id,
                          })
                        }
                      >
                        Acknowledge cleanup
                        <Icon name="arrow" />
                      </button>
                    </div>
                  </div>
                </Show>
                <div class="detail-actions">
                  <Show
                    when={data().kind === "service"}
                    fallback={
                      <>
                        {button(
                          "Run now",
                          "play",
                          {
                            operation: "run_now",
                            title: "",
                            description: "",
                            label: "Run now",
                          },
                          false,
                          blocked(),
                          true,
                        )}
                        {enabled()
                          ? button(
                              "Pause",
                              "pause",
                              {
                                operation: "pause",
                                title: "Pause this schedule?",
                                description:
                                  "Future triggers and queued automatic work will stop. Active and accepted manual runs can finish.",
                                label: "Pause",
                              },
                              true,
                            )
                          : button(
                              "Resume",
                              "play",
                              {
                                operation: "resume",
                                title: "",
                                description: "",
                                label: "Resume",
                              },
                              false,
                              blocked(),
                            )}
                        {button(
                          "Cancel run",
                          "stop",
                          {
                            operation: "cancel",
                            title: "Cancel active and queued work?",
                            description:
                              "The active process group will stop and queued work will be cleared. Future automatic triggers remain enabled unless you pause them.",
                            label: "Cancel run",
                            danger: true,
                          },
                          true,
                          data().value.observed.state !== "running" &&
                            !hasPending(),
                        )}
                      </>
                    }
                  >
                    {button(
                      "Start",
                      "play",
                      {
                        operation: "start",
                        title: "",
                        description: "",
                        label: "Start",
                      },
                      false,
                      blocked() || data().value.observed.state === "running",
                      true,
                    )}
                    {button(
                      "Stop",
                      "stop",
                      {
                        operation: "stop",
                        title: "Stop this service?",
                        description:
                          "The active process group will stop. Desired state becomes stopped and automatic retries are disabled.",
                        label: "Stop",
                        danger: true,
                      },
                      true,
                      data().value.observed.state === "stopped",
                    )}
                    {button(
                      "Restart",
                      "refresh",
                      {
                        operation: "restart",
                        title: "Restart this service?",
                        description:
                          "The current execution will stop before a replacement starts. This may interrupt work in progress.",
                        label: "Restart",
                      },
                      true,
                      blocked(),
                    )}
                  </Show>
                </div>
                <Show when={loaded.error}>
                  <p class="form-error" role="alert">
                    This detail could not refresh. {errorMessage(loaded.error)}
                  </p>
                </Show>
                <Show when={error() && !confirm()}>
                  <p class="form-error" role="alert">
                    {error()}
                  </p>
                </Show>
                <div
                  class="detail-tabs"
                  role="tablist"
                  aria-label="Workload details"
                  onKeyDown={(e) => {
                    if (
                      !["ArrowLeft", "ArrowRight", "Home", "End"].includes(
                        e.key,
                      )
                    )
                      return;
                    e.preventDefault();
                    const next =
                      e.key === "Home"
                        ? "activity"
                        : e.key === "End"
                          ? "definition"
                          : tab() === "activity"
                            ? "definition"
                            : "activity";
                    setTab(next);
                    document.getElementById(`${tabId}-${next}`)?.focus();
                  }}
                >
                  <button
                    role="tab"
                    id={`${tabId}-activity`}
                    aria-controls={`${tabId}-panel`}
                    tabIndex={tab() === "activity" ? 0 : -1}
                    aria-selected={tab() === "activity"}
                    onClick={() => setTab("activity")}
                  >
                    Activity
                  </button>
                  <button
                    role="tab"
                    id={`${tabId}-definition`}
                    aria-controls={`${tabId}-panel`}
                    tabIndex={tab() === "definition" ? 0 : -1}
                    aria-selected={tab() === "definition"}
                    onClick={() => setTab("definition")}
                  >
                    Definition
                  </button>
                </div>
                <div
                  role="tabpanel"
                  id={`${tabId}-panel`}
                  aria-labelledby={`${tabId}-${tab()}`}
                  tabIndex={0}
                >
                  <Show
                    when={tab() === "activity"}
                    fallback={<Definition data={data()} />}
                  >
                    <div class="detail-facts">
                      <div>
                        <span>
                          {data().kind === "service"
                            ? "Desired state"
                            : "Automatic triggers"}
                        </span>
                        <strong>{desiredLabel()}</strong>
                      </div>
                      <div>
                        <span>
                          {data().kind === "service"
                            ? "Latest result"
                            : "Next automatic run"}
                        </span>
                        <strong>{nextLabel()}</strong>
                      </div>
                    </div>
                    <Show when={hasPending()}>
                      <p class="inline-notice">
                        One invocation is queued. Active work finishes before it
                        starts.
                      </p>
                    </Show>
                    <div class="activity-heading">
                      <h3>Execution history</h3>
                      <span class="subtle">
                        Latest {data().history.length} of up to 100
                      </span>
                    </div>
                    <Show
                      when={data().history.length}
                      fallback={
                        <div class="empty-activity">
                          <Icon name="clock" />
                          <h3>No executions yet</h3>
                          <p>
                            {data().kind === "service"
                              ? "Start this service to begin its history."
                              : "Run it now, or resume automatic triggers."}
                          </p>
                        </div>
                      }
                    >
                      <div class="history-list">
                        <For each={data().history}>
                          {(run) => (
                            <button
                              class={`history-row ${selectedRun()?.id === run.id ? "selected" : ""}`}
                              onClick={() => void showLogs(run)}
                              aria-label={`View logs for execution ${run.id}`}
                            >
                              <span
                                class={`run-symbol ${run.outcome?.type === "exited" && "code" in run.outcome && run.outcome.code === 0 ? "green" : run.outcome ? "muted" : "blue"}`}
                              >
                                <Icon name={run.outcome ? "check" : "play"} />
                              </span>
                              <span>
                                <strong>Execution #{run.id}</strong>
                                <small>{timestamp(run.started_at_ms)}</small>
                              </span>
                              <span class="history-outcome">
                                {outcome(run)}
                              </span>
                              <Icon name="chevron" />
                            </button>
                          )}
                        </For>
                      </div>
                    </Show>
                    <Show when={selectedRun()}>
                      <section class="logs-panel">
                        <div>
                          <h3>
                            <Icon name="terminal" />
                            Output · #{selectedRun()!.id}
                          </h3>
                          <button
                            class="text-button"
                            disabled={logsBusy()}
                            onClick={() => void showLogs(selectedRun()!)}
                          >
                            <Icon name="refresh" />
                            Refresh
                          </button>
                        </div>
                        <p>
                          Last 64 KiB of each retained prefix. Sensitive output
                          stays in this tab.
                        </p>
                        <Show when={logsBusy()}>
                          <p class="subtle">Loading output…</p>
                        </Show>
                        <Show when={logsError()}>
                          <p class="form-error" role="alert">
                            {logsError()}
                          </p>
                        </Show>
                        <Show when={logs()}>
                          <h4>STDOUT</h4>
                          <pre data-testid="stdout">
                            {logs()!.stdout || "No stdout captured."}
                          </pre>
                          <h4>STDERR</h4>
                          <pre data-testid="stderr">
                            {logs()!.stderr || "No stderr captured."}
                          </pre>
                        </Show>
                      </section>
                    </Show>
                  </Show>
                </div>
              </>
            );
          }}
        </Show>
      </Dialog>
      <Show when={confirm()}>
        {(action) => (
          <Dialog
            title={action().title}
            close={() => {
              if (!busy()) setConfirm(undefined);
            }}
            class="confirm-dialog"
          >
            <span class={`confirm-symbol ${action().danger ? "danger" : ""}`}>
              <Icon name={action().danger ? "alert" : "refresh"} />
            </span>
            <h2>{action().title}</h2>
            <p class="dialog-description">{action().description}</p>
            <Show when={action().ack}>
              <label class="ack-checkbox">
                <input
                  type="checkbox"
                  checked={cleaned()}
                  onChange={(e) => setCleaned(e.currentTarget.checked)}
                />
                I inspected and cleaned up execution #{action().ack}.
              </label>
            </Show>
            <Show when={error()}>
              <p class="form-error" role="alert">
                {error()}
              </p>
            </Show>
            <Show when={loaded.error}>
              <p class="form-error" role="alert">
                The workload detail could not refresh. Controls are disabled
                until it reconnects.{" "}
                <button
                  class="text-button"
                  onClick={() => void refreshDetail()}
                >
                  Retry detail
                </button>
              </p>
            </Show>
            <div class="dialog-footer">
              <button
                class="button"
                disabled={busy()}
                onClick={() => setConfirm(undefined)}
              >
                Keep as is
              </button>
              <button
                class={`button ${action().danger ? "destructive" : "primary"}`}
                disabled={unavailable() || (!!action().ack && !cleaned())}
                onClick={() => void perform(action())}
              >
                {busy() ? "Applying…" : action().label}
              </button>
            </div>
          </Dialog>
        )}
      </Show>
    </>
  );
}
function Definition(props: { data: Detail }) {
  return (
    <div class="definition-panel">
      <h3>
        <Icon name="terminal" />
        Execution
      </h3>
      <dl>
        <dt>Executable</dt>
        <dd>
          <code>{props.data.definition.command[0]}</code>
        </dd>
        <dt>Arguments</dt>
        <dd>
          <pre>
            {props.data.definition.command.slice(1).join("\n") || "None"}
          </pre>
        </dd>
        <dt>Working directory</dt>
        <dd>
          <code>{props.data.definition.working_directory}</code>
        </dd>
      </dl>
      <Show
        when={props.data.kind === "service"}
        fallback={
          <Show when={"trigger" in props.data.definition}>
            <h3>
              <Icon name="clock" />
              Timing & policies
            </h3>
            <dl>
              <dt>Trigger</dt>
              <dd>
                {"trigger" in props.data.definition
                  ? cadence(props.data.definition.trigger)
                  : ""}
              </dd>
              <dt>Overlap</dt>
              <dd>
                {"overlap" in props.data.definition &&
                props.data.definition.overlap === "skip"
                  ? "Skip when busy"
                  : "Queue one latest run"}
              </dd>
              <dt>Missed runs</dt>
              <dd>
                {"missed" in props.data.definition &&
                props.data.definition.missed === "skip"
                  ? "Skip expired occurrences"
                  : "Catch up once"}
              </dd>
              <dt>Grace window</dt>
              <dd>
                {"grace_ms" in props.data.definition
                  ? `${props.data.definition.grace_ms} ms`
                  : ""}
              </dd>
            </dl>
          </Show>
        }
      >
        <dl>
          <dt>Restart policy</dt>
          <dd>
            {"restart" in props.data.definition
              ? props.data.definition.restart.replaceAll("_", " ")
              : ""}
          </dd>
          <dt>Restart delay</dt>
          <dd>
            {"restart_delay_ms" in props.data.definition
              ? `${props.data.definition.restart_delay_ms} ms`
              : ""}
          </dd>
        </dl>
      </Show>
      <p class="field-note">
        Commands receive the fixed system PATH. Arguments are literal; a shell
        runs only when explicitly configured. This workload belongs to Rhythm.
      </p>
    </div>
  );
}
