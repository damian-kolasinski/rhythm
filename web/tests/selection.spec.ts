import { test, expect } from "@playwright/test";
import type { Schedule } from "../src/api/contracts";
import { selectSchedules } from "../src/schedules/selection";
import { scheduleRoute, detailHash } from "../src/schedules/navigation";

const paused: Schedule = {
  id: "sample-alpha",
  enabled: false,
  observed: { state: "idle" },
  latest_execution: null,
  next_run_at_ms: null,
  pending: null,
  last_decision: null,
};
const running: Schedule = {
  ...paused,
  id: "sample-running",
  enabled: true,
  observed: { state: "running", execution_id: 1 },
  next_run_at_ms: 3000,
};
const failed: Schedule = {
  ...paused,
  id: "sample-failure",
  latest_execution: {
    id: 2,
    schedule_id: "sample-failure",
    invocation: { source: "manual", requested_at_ms: 1000 },
    started_at_ms: 1000,
    finished_at_ms: 2000,
    outcome: { type: "exited", code: 4 },
    recovery_acknowledged_at_ms: null,
  },
};
const enabled: Schedule = {
  ...paused,
  id: "sample-report",
  enabled: true,
  next_run_at_ms: 3000,
};
const uncertain: Schedule = {
  ...enabled,
  id: "sample-uncertain",
  observed: { state: "needs_attention" },
};
const schedules = [running, enabled, paused, uncertain, failed];

test("schedule selection puts attention first without mutating input", () => {
  expect(selectSchedules(schedules, "all", "")).toEqual([
    failed,
    uncertain,
    paused,
    enabled,
    running,
  ]);
  expect(schedules).toEqual([running, enabled, paused, uncertain, failed]);
  expect(selectSchedules([], "all", "")).toEqual([]);
});
test("schedule selection combines case-insensitive search and state", () => {
  expect(selectSchedules(schedules, "attention", "")).toEqual([
    failed,
    uncertain,
  ]);
  expect(selectSchedules(schedules, "attention", "FAIL")).toEqual([failed]);
  expect(selectSchedules(schedules, "running", "")).toEqual([running]);
  expect(selectSchedules(schedules, "paused", "")).toEqual([failed, paused]);
  expect(selectSchedules(schedules, "paused", "alpha")).toEqual([paused]);
  expect(selectSchedules(schedules, "all", "missing")).toEqual([]);
});
test("schedule detail routes accept bounded identifiers and reject malformed fragments", () => {
  expect(scheduleRoute("#/sample-report")).toBe("sample-report");
  expect(scheduleRoute("#/sample%2Dreport")).toBe("sample-report");
  expect(detailHash("sample-report")).toBe("/sample-report");
  for (const hash of [
    "",
    "#/",
    "#/services/sample-report",
    "#/schedules/sample-report",
    "#/%E0%A4%A",
    "#/../sample",
    "#/" + "a".repeat(64),
  ])
    expect(scheduleRoute(hash)).toBeUndefined();
});
