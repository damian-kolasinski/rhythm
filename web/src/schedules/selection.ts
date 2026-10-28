import type { Schedule } from "../api/contracts";
import { needsAttention } from "./presentation";
export type StateFilter = "all" | "running" | "attention" | "paused";

export function selectSchedules(
  schedules: Schedule[],
  state: StateFilter,
  search: string,
): Schedule[] {
  const query = search.toLowerCase();
  return schedules
    .filter(
      (schedule) =>
        schedule.id.toLowerCase().includes(query) &&
        (state === "all" ||
          (state === "running" && schedule.observed.state === "running") ||
          (state === "attention" && needsAttention(schedule)) ||
          (state === "paused" && !schedule.enabled)),
    )
    .sort(
      (a, b) =>
        Number(needsAttention(b)) - Number(needsAttention(a)) ||
        a.id.localeCompare(b.id),
    );
}
