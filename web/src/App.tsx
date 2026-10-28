import { createMemo, createSignal, Show, onMount, onCleanup } from "solid-js";
import { createApi } from "./api/client";
import { createWorkspace } from "./workloads/workspace";
import { items } from "./workloads/presentation";
import {
  selectWork,
  type KindFilter,
  type StateFilter,
} from "./workloads/selection";
import { workRoute, listHash } from "./workloads/navigation";
import { Icon } from "./components/Icon";
import { WorkTable } from "./components/WorkTable";
import { WorkDetail } from "./components/WorkDetail";

export default function App() {
  const api = createApi();
  const data = createWorkspace(api);
  const initial = workRoute(location.hash);
  const [current, setCurrent] = createSignal(initial);
  const [kind, setKind] = createSignal<KindFilter>(
    initial.detail ? "all" : initial.filter,
  );
  const [search, setSearch] = createSignal("");
  const [state, setState] = createSignal<StateFilter>("all");
  const [toast, setToast] = createSignal("");
  let searchInput: HTMLInputElement | undefined;
  let toastTimer: ReturnType<typeof setTimeout>;
  const all = createMemo(() =>
    items(data.snapshot()?.services ?? [], data.snapshot()?.schedules ?? []),
  );
  const filtered = createMemo(() =>
    selectWork(all(), kind(), state(), search()),
  );

  function notify(message: string) {
    setToast(message);
    clearTimeout(toastTimer);
    toastTimer = setTimeout(() => setToast(""), 4500);
    void data.refresh();
  }
  onMount(() => {
    const change = () => {
      const next = workRoute(location.hash);
      setCurrent(next);
      if (!next.detail) setKind(next.filter);
    };
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
        Skip to work
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
          <h1>Work</h1>
          <span class="subtle">{all().length} workloads</span>
        </div>
        <Show when={data.error()}>
          <div class="error-banner" role="alert">
            <Icon name="alert" />
            <div>
              <strong>
                {data.snapshot()
                  ? "Connection lost · showing the last snapshot"
                  : "Couldn’t load your work"}
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
              <p>Loading work…</p>
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
                  aria-label="Search work"
                  placeholder="Search work…"
                  value={search()}
                  onInput={(e) => setSearch(e.currentTarget.value)}
                />
                <kbd>⌘ K</kbd>
              </label>
              <select
                aria-label="Filter kind"
                value={kind()}
                onChange={(e) => {
                  location.hash = listHash(e.currentTarget.value as KindFilter);
                }}
              >
                <option value="all">All work</option>
                <option value="service">Services</option>
                <option value="schedule">Schedules</option>
              </select>
              <select
                aria-label="Filter state"
                value={state()}
                onChange={(e) => setState(e.currentTarget.value as StateFilter)}
              >
                <option value="all">All states</option>
                <option value="running">Running</option>
                <option value="attention">Needs attention</option>
                <option value="paused">Stopped or paused</option>
              </select>
            </div>
            <WorkTable
              items={filtered()}
              filtered={!!search() || kind() !== "all" || state() !== "all"}
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
      <Show when={current().detail}>
        {(detail) => (
          <WorkDetail
            kind={detail().kind}
            id={detail().id}
            api={api}
            offline={!!data.error()}
            close={() => {
              location.hash = listHash(kind());
            }}
            changed={notify}
          />
        )}
      </Show>
    </div>
  );
}
