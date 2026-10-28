import type { Kind } from "./presentation";
import type { KindFilter } from "./selection";

export function workRoute(hash: string): {
  filter: KindFilter;
  detail?: { kind: Kind; id: string };
} {
  const [section, encoded] = hash.replace(/^#\/?/, "").split("/");
  const kind =
    section === "services"
      ? "service"
      : section === "schedules"
        ? "schedule"
        : undefined;
  if (!kind) return { filter: "all" };
  if (encoded) {
    try {
      return {
        filter: kind,
        detail: { kind, id: decodeURIComponent(encoded) },
      };
    } catch {
      // A malformed deep link still opens the list.
    }
  }
  return { filter: kind };
}

export function listHash(kind: KindFilter): string {
  return kind === "all" ? "/" : `/${kind}s`;
}
