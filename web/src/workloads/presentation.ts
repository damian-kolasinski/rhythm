import type { Execution, Service, Schedule, Trigger } from "../api/contracts";
export type Kind = "service" | "schedule";
export type Item = { kind: Kind; value: Service | Schedule };
export function isUncertain(execution: Execution | null): boolean {
  return (
    !!execution &&
    execution.recovery_acknowledged_at_ms === null &&
    ["monitor_lost", "output_unavailable"].includes(
      execution.outcome?.type ?? "",
    )
  );
}
export function failed(execution: Execution | null): boolean {
  const o = execution?.outcome;
  return (
    !!o &&
    (o.type === "exited"
      ? "code" in o && o.code !== 0
      : [
          "signalled",
          "spawn_failed",
          "monitor_lost",
          "output_unavailable",
        ].includes(o.type))
  );
}
export function needsAttention(item: Item): boolean {
  return (
    item.value.observed.state === "needs_attention" ||
    failed(item.value.latest_execution)
  );
}
export function status(item: Item): { label: string; tone: string } {
  const state = item.value.observed.state;
  if (state === "needs_attention")
    return { label: "Needs attention", tone: "danger" };
  if (state === "running") return { label: "Running", tone: "green" };
  if (state === "recovering") return { label: "Recovering", tone: "amber" };
  if (state === "waiting") return { label: "Retry queued", tone: "amber" };
  if (failed(item.value.latest_execution))
    return { label: "Failed", tone: "danger" };
  if (item.kind === "service")
    return {
      label:
        (item.value as Service).desired === "stopped" ? "Stopped" : "Exited",
      tone: "muted",
    };
  return {
    label: (item.value as Schedule).enabled ? "Scheduled" : "Paused",
    tone: (item.value as Schedule).enabled ? "blue" : "muted",
  };
}
export function outcome(execution: Execution | null): string {
  const o = execution?.outcome;
  if (!execution) return "No executions yet";
  if (!o) return "Execution active";
  if (o.type === "exited")
    return "code" in o && o.code === 0
      ? "Completed"
      : `Exit ${"code" in o ? o.code : "unknown"}`;
  if (o.type === "signalled")
    return `Signal ${"signal" in o ? o.signal : "unknown"}`;
  return (
    (
      {
        stopped: "Stopped",
        interrupted: "Interrupted",
        spawn_failed: "Could not start",
        monitor_lost: "Monitor lost",
        output_unavailable: "Output unavailable",
      } as Record<string, string>
    )[o.type] ?? "Unknown outcome"
  );
}
export function when(ms: number | null, now = Date.now()): string {
  if (ms === null) return "—";
  const delta = ms - now;
  if (delta >= 0 && delta < 60000)
    return delta < 1000 ? "Due now" : `in ${Math.ceil(delta / 1000)}s`;
  if (delta >= 0 && delta < 3600000) return `in ${Math.ceil(delta / 60000)}m`;
  if (delta >= 0 && delta < 86400000)
    return `in ${Math.floor(delta / 3600000)}h ${Math.floor((delta % 3600000) / 60000)}m`;
  if (delta < 0 && delta > -60000) return "Just now";
  return new Intl.DateTimeFormat("en", {
    month: "short",
    day: "numeric",
    hour: "2-digit",
    minute: "2-digit",
    hour12: false,
  }).format(new Date(ms));
}
export function timestamp(ms: number | null): string {
  return ms === null
    ? "—"
    : new Intl.DateTimeFormat("en", {
        dateStyle: "medium",
        timeStyle: "medium",
      }).format(new Date(ms));
}
export function cadence(trigger: Trigger): string {
  if (trigger.type === "interval")
    return trigger.every_ms < 60000
      ? `Every ${trigger.every_ms / 1000}s`
      : trigger.every_ms < 3600000
        ? `Every ${trigger.every_ms / 60000}m`
        : `Every ${trigger.every_ms / 3600000}h`;
  const time = `${String(trigger.hour).padStart(2, "0")}:${String(trigger.minute).padStart(2, "0")}`;
  const days = ["", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];
  return `${trigger.type === "daily" ? "Daily" : trigger.weekdays.map((d) => days[d]).join(", ")} at ${time} · ${trigger.timezone}`;
}
export function items(services: Service[], schedules: Schedule[]): Item[] {
  return [
    ...services.map((value) => ({ kind: "service" as const, value })),
    ...schedules.map((value) => ({ kind: "schedule" as const, value })),
  ];
}
