export type IconName =
  | "wave"
  | "clock"
  | "search"
  | "arrow"
  | "close"
  | "check"
  | "alert"
  | "play"
  | "pause"
  | "refresh"
  | "stop"
  | "terminal"
  | "chevron";
const paths: Record<IconName, string[]> = {
  wave: ["M2 12h4l3-8 5 16 3-8h5"],
  clock: ["M12 22a10 10 0 1 0 0-20 10 10 0 0 0 0 20", "M12 6v6l4 2"],
  search: ["M21 21l-5-5", "M10 18a8 8 0 1 0 0-16 8 8 0 0 0 0 16"],
  arrow: ["M5 12h14M14 7l5 5-5 5"],
  close: ["M6 6l12 12M18 6 6 18"],
  check: ["m5 12 4 4L19 6"],
  alert: [
    "M12 8v5M12 17h.01",
    "M10 3 2 18a2 2 0 0 0 2 3h16a2 2 0 0 0 2-3L14 3a2 2 0 0 0-4 0",
  ],
  play: ["m8 4 12 8-12 8z"],
  pause: ["M7 5h3v14H7zM14 5h3v14h-3z"],
  refresh: [
    "M21 4v6h-6M3 20v-6h6",
    "M20 9a8 8 0 0 0-14-4L3 8M4 15a8 8 0 0 0 14 4l3-3",
  ],
  stop: ["M5 5h14v14H5z"],
  terminal: ["m4 5 6 6-6 6M13 18h7"],
  chevron: ["m9 5 7 7-7 7"],
};
export function Icon(props: { name: IconName; class?: string }) {
  return (
    <svg
      class={props.class ?? ""}
      width="20"
      height="20"
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      stroke-width="1.7"
      stroke-linecap="round"
      stroke-linejoin="round"
      aria-hidden="true"
    >
      {paths[props.name].map((d) => (
        <path d={d} />
      ))}
    </svg>
  );
}
