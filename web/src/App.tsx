import { createMemo, createSignal, Show, onMount, onCleanup } from "solid-js";
import { createApi } from "./api/client";
import { createWorkspace } from "./schedules/workspace";
import { selectSchedules, type StateFilter } from "./schedules/selection";
import { scheduleRoute } from "./schedules/navigation";
import { Icon } from "./components/Icon";
import { ScheduleTable } from "./components/ScheduleTable";
import { ScheduleDetail } from "./components/ScheduleDetail";

export default function App() {
  const api = createApi();
  const data = createWorkspace(api);
  const [current, setCurrent] = createSignal(scheduleRoute(location.hash));
  const [search, setSearch] = createSignal("");
  const [state, setState] = createSignal<StateFilter>("all");
  const [toast, setToast] = createSignal("");
  let searchInput: HTMLInputElement | undefined;
  let toastTimer: ReturnType<typeof setTimeout>;
  const all = () => data.snapshot()?.schedules ?? [];
  const filtered = createMemo(() => selectSchedules(all(), state(), search()));

  function notify(message: string) {
    setToast(message);
    clearTimeout(toastTimer);
    toastTimer = setTimeout(() => setToast(""), 4500);
    void data.refresh();
  }
  onMount(() => {
    const change = () => setCurrent(scheduleRoute(location.hash));
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
        Skip to schedules
      </a>
      <header class="topbar">
        <span class="brand">
          <Icon name="wave" />
          Rhythm
        </span>
        <div class="topbar-right">
          <span class="connection" role="status">
            <span
              classList={{ "connection-dot": true, offline: !!data.error() }}
            />
            {data.error()
              ? "Connection lost"
              : data.loading()
                ? "Connecting…"
                : "Connected"}
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
        <Show when={data.snapshot()?.demo}>
          <div class="demo-banner">
            <Icon name="play" />
            <strong>Demo workspace</strong>
            <span>Synthetic work · disposable state</span>
          </div>
        </Show>
        <div class="page-heading">
          <h1>Schedules</h1>
          <span class="subtle">
            {all().length} {all().length === 1 ? "schedule" : "schedules"}
          </span>
        </div>
        <Show when={data.error()}>
          <div class="error-banner" role="alert">
            <Icon name="alert" />
            <div>
              <strong>
                {data.snapshot()
                  ? "Connection lost · showing the last snapshot"
                  : "Couldn’t load your schedules"}
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
              <div class="skeleton-table" />
              <p>Loading schedules…</p>
            </div>
          }
        >
          <Show when={data.snapshot()}>
            <div class="work-toolbar">
              <label class="search-field">
                <Icon name="search" />
                <input
                  ref={searchInput}
                  type="search"
                  aria-label="Search schedules"
                  placeholder="Search schedules…"
                  value={search()}
                  onInput={(e) => setSearch(e.currentTarget.value)}
                />
                <kbd>⌘ K</kbd>
              </label>
              <select
                aria-label="Filter state"
                value={state()}
                onChange={(e) => setState(e.currentTarget.value as StateFilter)}
              >
                <option value="all">All states</option>
                <option value="running">Running</option>
                <option value="attention">Needs attention</option>
                <option value="paused">Paused</option>
              </select>
            </div>
            <ScheduleTable
              schedules={filtered()}
              filtered={!!search() || state() !== "all"}
            />
          </Show>
        </Show>
      </main>
      <div class="toast-region" role="status" aria-live="polite">
        <Show when={toast()}>
          <div class="toast">
            <Icon name="check" />
            {toast()}
          </div>
        </Show>
      </div>
      <Show when={current()}>
        {(id) => (
          <ScheduleDetail
            id={id()}
            api={api}
            offline={!!data.error()}
            close={() => {
              location.hash = "/";
            }}
            changed={notify}
          />
        )}
      </Show>
    </div>
  );
}
