import { test, expect } from "@playwright/test";
import type { Item } from "../src/workloads/presentation";
import { selectWork } from "../src/workloads/selection";
import { workRoute, listHash } from "../src/workloads/navigation";

const stopped: Item = {
  kind: "service",
  value: {
    id: "sample-alpha",
    desired: "stopped",
    observed: { state: "stopped" },
    latest_execution: null,
  },
};
const running: Item = {
  kind: "service",
  value: {
    id: "sample-running",
    desired: "running",
    observed: { state: "running", execution_id: 1 },
    latest_execution: null,
  },
};
const failed: Item = {
  kind: "service",
  value: {
    id: "sample-failure",
    desired: "running",
    observed: { state: "exited" },
    latest_execution: {
      id: 2,
      started_at_ms: 1000,
      finished_at_ms: 2000,
      outcome: { type: "exited", code: 4 },
      recovery_acknowledged_at_ms: null,
    },
  },
};
const paused: Item = {
  kind: "schedule",
  value: {
    id: "sample-alpha",
    enabled: false,
    observed: { state: "idle" },
    latest_execution: null,
    next_run_at_ms: null,
    pending: null,
    last_decision: null,
  },
};
const enabled: Item = {
  kind: "schedule",
  value: {
    ...paused.value,
    id: "sample-report",
    enabled: true,
    next_run_at_ms: 3000,
  } as Extract<Item["value"], { enabled: boolean }>,
};
const uncertain: Item = {
  kind: "schedule",
  value: {
    ...enabled.value,
    id: "sample-uncertain",
    observed: { state: "needs_attention" },
  },
};
const work = [running, enabled, stopped, paused, uncertain, failed];

test("work selection puts attention first without merging same-name namespaces or mutating input", () => {
  const selected = selectWork(work, "all", "all", "");
  expect(selected).toEqual([
    failed,
    uncertain,
    paused,
    stopped,
    enabled,
    running,
  ]);
  expect(work).toEqual([running, enabled, stopped, paused, uncertain, failed]);
  expect(selectWork([], "all", "all", "")).toEqual([]);
});

test("work selection combines case-insensitive search, kind and state", () => {
  expect(selectWork(work, "all", "attention", "")).toEqual([failed, uncertain]);
  expect(selectWork(work, "service", "attention", "FAIL")).toEqual([failed]);
  expect(selectWork(work, "schedule", "attention", "FAIL")).toEqual([]);
  expect(selectWork(work, "all", "running", "")).toEqual([running]);
  expect(selectWork(work, "all", "paused", "")).toEqual([paused, stopped]);
  expect(selectWork(work, "schedule", "paused", "alpha")).toEqual([paused]);
});

test("work routes preserve existing service and schedule links and reject malformed fragments", () => {
  expect(workRoute("#/services/sample-alpha")).toEqual({
    filter: "service",
    detail: { kind: "service", id: "sample-alpha" },
  });
  expect(workRoute("#/schedules/sample%2Dalpha")).toEqual({
    filter: "schedule",
    detail: { kind: "schedule", id: "sample-alpha" },
  });
  expect(workRoute("#/schedules")).toEqual({ filter: "schedule" });
  expect(workRoute("#/services/%E0%A4%A")).toEqual({ filter: "service" });
  expect(workRoute("#/unknown/sample-alpha")).toEqual({ filter: "all" });
  expect(workRoute("")).toEqual({ filter: "all" });
  expect(listHash("all")).toBe("/");
  expect(listHash("service")).toBe("/services");
  expect(listHash("schedule")).toBe("/schedules");
});
