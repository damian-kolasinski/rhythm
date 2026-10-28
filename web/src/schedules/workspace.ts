import { createSignal, onCleanup, onMount } from "solid-js";
import { createStore, reconcile } from "solid-js/store";
import { workspaceInfo, type Api } from "../api/client";
import { errorMessage } from "../api/errors";
import {
  scheduleListSchema,
  statusSchema,
  type Schedule,
} from "../api/contracts";
export type Snapshot = {
  schedules: Schedule[];
  version: string;
  demo: boolean;
};
export function createWorkspace(api: Api) {
  const [state, setState] = createStore<{ snapshot?: Snapshot }>({});
  const snapshot = () => state.snapshot;
  const [error, setError] = createSignal("");
  const [loading, setLoading] = createSignal(true);
  let pending: Promise<void> | undefined;
  let disposed = false;
  async function load() {
    try {
      const [schedules, status, info] = await Promise.all([
        api({ operation: "list" }, scheduleListSchema),
        api({ operation: "status" }, statusSchema),
        workspaceInfo(),
      ]);
      if (!disposed) {
        setState(
          "snapshot",
          reconcile(
            {
              schedules: schedules.schedules,
              version: status.version,
              demo: info.demo,
            },
            { key: "id" },
          ),
        );
        setError("");
      }
    } catch (e) {
      if (!disposed) setError(errorMessage(e));
    } finally {
      if (!disposed) setLoading(false);
    }
  }
  function refresh() {
    return (pending ??= load().finally(() => {
      pending = undefined;
    }));
  }
  onMount(() => {
    void refresh();
    const timer = setInterval(() => void refresh(), 2000);
    onCleanup(() => clearInterval(timer));
  });
  onCleanup(() => {
    disposed = true;
  });
  return { snapshot, error, loading, refresh };
}
