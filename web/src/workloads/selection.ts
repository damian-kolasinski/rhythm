import { needsAttention, type Item, type Kind } from "./presentation";

export type KindFilter = "all" | Kind;
export type StateFilter = "all" | "running" | "attention" | "paused";

export function selectWork(
  work: Item[],
  kind: KindFilter,
  state: StateFilter,
  search: string,
): Item[] {
  const query = search.toLowerCase();
  return work
    .filter(
      (item) =>
        (kind === "all" || item.kind === kind) &&
        item.value.id.toLowerCase().includes(query) &&
        (state === "all" ||
          (state === "running" && item.value.observed.state === "running") ||
          (state === "attention" && needsAttention(item)) ||
          (state === "paused" &&
            ("enabled" in item.value
              ? !item.value.enabled
              : item.value.desired === "stopped"))),
    )
    .sort(
      (a, b) =>
        Number(needsAttention(b)) - Number(needsAttention(a)) ||
        a.value.id.localeCompare(b.value.id) ||
        a.kind.localeCompare(b.kind),
    );
}
