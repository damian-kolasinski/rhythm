import {
  createMemo,
  createSignal,
  Show,
  For,
  onMount,
  onCleanup,
} from "solid-js";
import { createApi, disconnect } from "./api/client";
import { readSession, clearSession, type Session } from "./session/storage";
import { createWorkspace } from "./workloads/workspace";
import {
  items,
  needsAttention,
  status,
  when,
  type Kind,
  type Item,
} from "./workloads/presentation";
import { Connect } from "./components/Connect";
import { Icon } from "./components/Icon";
import { WorkTable } from "./components/WorkTable";
import { WorkDetail } from "./components/WorkDetail";
import { CreateWork } from "./components/CreateWork";
type Page = "overview" | "services" | "schedules";
function route() {
  const parts = location.hash.replace(/^#\/?/, "").split("/");
  const page: Page =
    parts[0] === "services"
      ? "services"
      : parts[0] === "schedules"
        ? "schedules"
        : "overview";
  let id: string | undefined;
  try {
    if (parts[1]) id = decodeURIComponent(parts[1]);
  } catch {}
  return { page, id };
}
export default function App() {
  const [session, setSession] = createSignal<Session | undefined>(
    readSession(),
  );
  const [expired, setExpired] = createSignal(false);
  function expire() {
    clearSession();
    setExpired(true);
    setSession(undefined);
  }
  return (
    <Show
      when={session()}
      fallback={
        <Connect
          connected={(s) => {
            setExpired(false);
            setSession(s);
          }}
          expired={expired()}
        />
      }
      keyed
    >
      {(active) => <Workspace session={active} expire={expire} />}
    </Show>
  );
}
function Workspace(props: { session: Session; expire: () => void }) {
  const api = createApi(props.session.token, props.expire);
  const data = createWorkspace(api);
  const [current, setCurrent] = createSignal(route());
  const [search, setSearch] = createSignal("");
  const [filter, setFilter] = createSignal("all");
  const [create, setCreate] = createSignal<Kind>();
  const [toast, setToast] = createSignal("");
  let searchInput: HTMLInputElement | undefined;
  let toastTimer: ReturnType<typeof setTimeout>;
  const all = createMemo(() =>
    items(data.snapshot()?.services ?? [], data.snapshot()?.schedules ?? []),
  );
  const attention = createMemo(() => all().filter(needsAttention));
  const filtered = createMemo(() =>
    all().filter(
      (item) =>
        item.value.id.toLowerCase().includes(search().toLowerCase()) &&
        (filter() === "all" ||
          (filter() === "running" && item.value.observed.state === "running") ||
          (filter() === "attention" && needsAttention(item)) ||
          (filter() === "paused" &&
            ("enabled" in item.value
              ? !item.value.enabled
              : item.value.desired === "stopped"))),
    ),
  );
  const services = createMemo(() =>
    filtered().filter((i) => i.kind === "service"),
  );
  const schedules = createMemo(() =>
    filtered().filter((i) => i.kind === "schedule"),
  );
  const upcoming = createMemo(() =>
    all()
      .filter(
        (i): i is Item & { value: import("./api/contracts").Schedule } =>
          i.kind === "schedule" &&
          "enabled" in i.value &&
          i.value.enabled &&
          i.value.next_run_at_ms !== null,
      )
      .sort(
        (a, b) => (a.value.next_run_at_ms ?? 0) - (b.value.next_run_at_ms ?? 0),
      ),
  );
  const running = createMemo(
    () => all().filter((i) => i.value.observed.state === "running").length,
  );
  function notify(message: string) {
    setToast(message);
    clearTimeout(toastTimer);
    toastTimer = setTimeout(() => setToast(""), 4500);
    void data.refresh();
  }
  function closeDetail() {
    location.hash = `/${current().page}`;
  }
  onMount(() => {
    const change = () => setCurrent(route());
    const key = (e: KeyboardEvent) => {
      if ((e.metaKey || e.ctrlKey) && e.key === "k") {
        e.preventDefault();
        searchInput?.focus();
      }
    };
    window.addEventListener("hashchange", change);
    window.addEventListener("keydown", key);
    onCleanup(() => {
      window.removeEventListener("hashchange", change);
      window.removeEventListener("keydown", key);
      clearTimeout(toastTimer);
    });
  });
  async function leave() {
    try {
      await disconnect(props.session.token);
    } finally {
      props.expire();
    }
  }
  const title = () =>
    ({
      overview: "Background work, in view.",
      services: "Services that stay with you.",
      schedules: "A place for recurring work.",
    })[current().page];
  return (
    <div class="app-shell">
      <a
        class="skip-link"
        href="#workspace-main"
        onClick={(e) => {
          e.preventDefault();
          document.getElementById("workspace-main")?.focus();
        }}
      >
        Skip to workspace
      </a>
      <aside class="sidebar">
        <a href="#/" class="brand">
          <span class="brand-mark">
            <Icon name="wave" />
          </span>
          Rhythm<span class="brand-dot">.</span>
        </a>
        <div class="workspace-label">
          <span class="workspace-avatar">R</span>
          <span>
            Local workspace<small>Background work</small>
          </span>
        </div>
        <p class="nav-label">WORKSPACE</p>
        <nav aria-label="Main navigation">
          {(
            [
              ["overview", "grid", "Overview"],
              ["services", "service", "Services"],
              ["schedules", "clock", "Schedules"],
            ] as const
          ).map(([page, icon, label]) => (
            <a
              href={`#/${page === "overview" ? "" : page}`}
              aria-label={label}
              classList={{ active: current().page === page }}
              aria-current={current().page === page ? "page" : undefined}
            >
              <Icon name={icon} />
              <span>{label}</span>
              <Show when={page !== "overview"}>
                <small>
                  {page === "services"
                    ? (data.snapshot()?.services.length ?? 0)
                    : (data.snapshot()?.schedules.length ?? 0)}
                </small>
              </Show>
            </a>
          ))}
        </nav>
        <div class="sidebar-bottom">
          <div class="local-note">
            <Icon name="wave" />
            <p>
              Your work. Your runtime.<small>Always within reach.</small>
            </p>
          </div>
          <div class="connection">
            <span
              classList={{ "connection-dot": true, offline: !!data.error() }}
            />
            <span>
              {data.error()
                ? "Connection lost"
                : data.loading()
                  ? "Connecting…"
                  : "Runtime connected"}
              <small>
                {data.snapshot()
                  ? `v${data.snapshot()!.version}`
                  : "Connecting…"}
              </small>
            </span>
            <button
              class="icon-button"
              aria-label="Disconnect browser"
              onClick={() => void leave()}
            >
              <Icon name="logout" />
            </button>
          </div>
        </div>
      </aside>
      <div class="main-shell">
        <header class="topbar">
          <div class="breadcrumbs">
            Workspace
            <Icon name="chevron" />
            <strong>
              {current().page === "overview"
                ? "Overview"
                : current().page === "services"
                  ? "Services"
                  : "Schedules"}
            </strong>
          </div>
          <div class="topbar-right">
            <span class="local-badge">
              <i />
              Local runtime
            </span>
            <button
              class="icon-button"
              aria-label="Refresh workspace"
              onClick={() => void data.refresh()}
            >
              <Icon name="refresh" />
            </button>
          </div>
        </header>
        <main id="workspace-main" class="workspace-main" tabIndex={-1}>
          <Show when={props.session.demo}>
            <div class="demo-banner">
              <span>
                <Icon name="play" />
                <strong>Demo workspace</strong>
                <span>
                  Synthetic work, real runtime. Changes stay in this disposable
                  workspace.
                </span>
              </span>
              <span class="demo-tag">DEMO</span>
            </div>
          </Show>
          <div class="page-heading">
            <div>
              <p class="eyebrow">
                {current().page === "overview"
                  ? "THE BIG PICTURE"
                  : current().page === "services"
                    ? "ONGOING WORK"
                    : "RECURRING WORK"}
              </p>
              <h1>{title()}</h1>
              <p>
                {current().page === "overview"
                  ? "See what’s running, what’s next, and what needs a look."
                  : current().page === "services"
                    ? "Control ongoing work without losing its history or context."
                    : "Plan the next run. Keep every execution in view."}
              </p>
            </div>
            <div class="new-actions">
              <button
                class="button"
                disabled={data.loading() || !!data.error()}
                onClick={() => setCreate("schedule")}
              >
                <Icon name="clock" />
                New schedule
              </button>
              <button
                class="button primary"
                disabled={data.loading() || !!data.error()}
                onClick={() => setCreate("service")}
              >
                <Icon name="plus" />
                New service
              </button>
            </div>
          </div>
          <Show when={data.error()}>
            <div class="error-banner" role="alert">
              <Icon name="alert" />
              <div>
                <strong>
                  {data.snapshot()
                    ? "Connection lost · showing the last snapshot"
                    : "Couldn’t load your workspace"}
                </strong>
                <p>{data.error()}</p>
              </div>
              <button class="button" onClick={() => void data.refresh()}>
                Retry
              </button>
            </div>
          </Show>
          <Show
            when={!data.loading()}
            fallback={
              <div class="loading-workspace" role="status">
                <div class="skeleton-metrics">
                  <i />
                  <i />
                  <i />
                  <i />
                </div>
                <div class="skeleton-table" />
                <p>Loading your workspace…</p>
              </div>
            }
          >
            <Show when={data.snapshot()}>
              <div class="metrics">
                <Metric
                  label="Running now"
                  value={running()}
                  caption="Active executions"
                  icon="play"
                  tone="green"
                />
                <Metric
                  label="Needs a look"
                  value={attention().length}
                  caption={
                    attention().length
                      ? "Inspect the latest outcome"
                      : "Nothing waiting for attention"
                  }
                  icon="alert"
                  tone={attention().length ? "amber" : "muted"}
                />
                <Metric
                  label="Upcoming"
                  value={upcoming().length}
                  caption="Enabled schedules"
                  icon="clock"
                  tone="blue"
                />
                <Metric
                  label="Paused schedules"
                  value={
                    data.snapshot()!.schedules.filter((s) => !s.enabled).length
                  }
                  caption="Ready when you are"
                  icon="pause"
                  tone="muted"
                />
              </div>
              <Show when={current().page === "overview"}>
                <div class="overview-panels">
                  <section
                    class={`attention-card ${attention().length ? "has-attention" : ""}`}
                  >
                    <div class="panel-title">
                      <span class="panel-icon">
                        <Icon name={attention().length ? "alert" : "check"} />
                      </span>
                      <h2>
                        {attention().length
                          ? "Worth a closer look"
                          : "A little breathing room"}
                      </h2>
                      <span class="subtle">
                        {attention().length
                          ? `${attention().length} workload${attention().length === 1 ? "" : "s"}`
                          : "All clear"}
                      </span>
                    </div>
                    <Show
                      when={attention().length}
                      fallback={
                        <p>
                          No failures or uncertain completions need attention
                          right now.
                        </p>
                      }
                    >
                      <For each={attention().slice(0, 2)}>
                        {(item) => (
                          <a
                            class="attention-row"
                            href={`#/${item.kind === "service" ? "services" : "schedules"}/${item.value.id}`}
                          >
                            <span>
                              <strong>{item.value.id}</strong>
                              <small>
                                {status(item).label} ·{" "}
                                {item.kind === "service"
                                  ? "Service"
                                  : "Schedule"}
                              </small>
                            </span>
                            <Icon name="arrow" />
                          </a>
                        )}
                      </For>
                    </Show>
                  </section>
                  <section class="upcoming-card">
                    <div class="panel-title">
                      <span class="panel-icon">
                        <Icon name="clock" />
                      </span>
                      <h2>On the horizon</h2>
                      <span class="subtle">Automatic runs</span>
                    </div>
                    <Show
                      when={upcoming().length}
                      fallback={
                        <p>
                          No automatic runs ahead. Resume a schedule when you’re
                          ready.
                        </p>
                      }
                    >
                      <For each={upcoming().slice(0, 3)}>
                        {(item) => (
                          <a
                            class="upcoming-row"
                            href={`#/schedules/${item.value.id}`}
                          >
                            <span>{item.value.id}</span>
                            <strong>{when(item.value.next_run_at_ms)}</strong>
                          </a>
                        )}
                      </For>
                    </Show>
                  </section>
                </div>
              </Show>
              <div class="work-toolbar">
                <label class="search-field">
                  <Icon name="search" />
                  <input
                    ref={searchInput}
                    type="search"
                    aria-label="Search work"
                    placeholder="Find work by name…"
                    value={search()}
                    onInput={(e) => setSearch(e.currentTarget.value)}
                  />
                  <kbd>⌘ K</kbd>
                </label>
                <select
                  aria-label="Filter state"
                  value={filter()}
                  onChange={(e) => setFilter(e.currentTarget.value)}
                >
                  <option value="all">All states</option>
                  <option value="running">Running</option>
                  <option value="attention">Needs a look</option>
                  <option value="paused">Stopped or paused</option>
                </select>
                <span class="toolbar-meta">{filtered().length} workloads</span>
              </div>
              <Show when={current().page !== "schedules"}>
                <WorkTable
                  title="Services"
                  subtitle="Ongoing work, with explicit desired state."
                  items={services()}
                  empty={
                    search() || filter() !== "all"
                      ? "No services match these filters."
                      : "No services yet. Give your first worker a home."
                  }
                  newWork={() => setCreate("service")}
                  unavailable={!!data.error()}
                />
              </Show>
              <Show when={current().page !== "services"}>
                <WorkTable
                  title="Schedules"
                  subtitle="Finite runs, on a plan you control."
                  items={schedules()}
                  empty={
                    search() || filter() !== "all"
                      ? "No schedules match these filters."
                      : "No schedules yet. Make room for recurring work."
                  }
                  newWork={() => setCreate("schedule")}
                  unavailable={!!data.error()}
                />
              </Show>
            </Show>
          </Show>
          <footer class="workspace-footer">
            <span>
              <Icon name="wave" />
              Built for the work that keeps going.
            </span>
            <span>
              Times in {Intl.DateTimeFormat().resolvedOptions().timeZone}
            </span>
          </footer>
        </main>
      </div>
      <div class="toast-region" role="status" aria-live="polite">
        <Show when={toast()}>
          <div class="toast">
            <Icon name="check" />
            {toast()}
          </div>
        </Show>
      </div>
      <Show when={create()}>
        {(kind) => (
          <CreateWork
            kind={kind()}
            api={api}
            unavailable={!data.snapshot() || !!data.error()}
            close={() => setCreate(undefined)}
            created={(id) => {
              const createdKind = kind();
              setCreate(undefined);
              notify(`Created ${id}`);
              location.hash = `/${createdKind === "service" ? "services" : "schedules"}/${id}`;
            }}
          />
        )}
      </Show>
      <Show when={current().id}>
        {(id) => (
          <WorkDetail
            kind={current().page === "schedules" ? "schedule" : "service"}
            id={id()}
            api={api}
            offline={!!data.error()}
            close={closeDetail}
            changed={notify}
          />
        )}
      </Show>
    </div>
  );
}
function Metric(props: {
  label: string;
  value: number;
  caption: string;
  icon: import("./components/Icon").IconName;
  tone: string;
}) {
  return (
    <section class="metric">
      <div>
        <span>{props.label}</span>
        <span class={`metric-icon ${props.tone}`}>
          <Icon name={props.icon} />
        </span>
      </div>
      <strong>
        {props.value}
        <span>{props.label === "Running now" ? "live" : ""}</span>
      </strong>
      <p>{props.caption}</p>
    </section>
  );
}
