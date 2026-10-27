import { createSignal, onCleanup, onMount } from "solid-js";
import { createStore, reconcile } from "solid-js/store";
import type { Api } from "../api/client";
import { errorMessage } from "../api/errors";
import {
  serviceListSchema,
  scheduleListSchema,
  statusSchema,
  type Service,
  type Schedule,
} from "../api/contracts";
export type Snapshot = {
  services: Service[];
  schedules: Schedule[];
  version: string;
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
      const [services, schedules, status] = await Promise.all([
        api({ operation: "list" }, serviceListSchema),
        api(
          { operation: "schedule", command: { operation: "list" } },
          scheduleListSchema,
        ),
        api({ operation: "status" }, statusSchema),
      ]);
      if (!disposed) {
        setState(
          "snapshot",
          reconcile(
            {
              services: services.services,
              schedules: schedules.response.schedules,
              version: status.version,
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
