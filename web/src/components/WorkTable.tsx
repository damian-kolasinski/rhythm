import { For, Show } from "solid-js";
import { status, outcome, when, type Item } from "../workloads/presentation";
import { Icon } from "./Icon";

export function WorkTable(props: { items: Item[]; filtered: boolean }) {
  return (
    <section class="work-section" aria-label="Work list">
      <Show
        when={props.items.length}
        fallback={
          <div class="empty-section">
            <p>
              {props.filtered
                ? "No work matches these filters."
                : "No work registered yet."}
            </p>
            <Show when={!props.filtered}>
              <p class="subtle">
                Create work with the CLI:{" "}
                <code>rhythm service create --file FILE</code> or{" "}
                <code>rhythm schedule create --file FILE</code>.
              </p>
            </Show>
          </div>
        }
      >
        <div class="table-scroll">
          <table aria-label="Work">
            <thead>
              <tr>
                <th>Name</th>
                <th>Status</th>
                <th class="latest-execution">Latest execution</th>
                <th class="next-run">Next run</th>
                <th>
                  <span class="sr-only">Open details</span>
                </th>
              </tr>
            </thead>
            <tbody>
              <For each={props.items}>
                {(item) => {
                  const state = () => status(item);
                  return (
                    <tr>
                      <td>
                        <a
                          class="work-name"
                          href={`#/${item.kind}s/${encodeURIComponent(item.value.id)}`}
                        >
                          <span class="work-icon">
                            <Icon
                              name={
                                item.kind === "service" ? "service" : "clock"
                              }
                            />
                          </span>
                          <span>
                            {item.value.id}
                            <small>
                              {item.kind === "service" ? "Service" : "Schedule"}
                            </small>
                          </span>
                        </a>
                      </td>
                      <td>
                        <span class={`status ${state().tone}`}>
                          <i />
                          {state().label}
                        </span>
                        <Show
                          when={"enabled" in item.value && item.value.enabled}
                        >
                          <small class="compact-next">
                            Next{" "}
                            {"next_run_at_ms" in item.value &&
                              when(item.value.next_run_at_ms)}
                          </small>
                        </Show>
                      </td>
                      <td class="latest-execution">
                        <span class="table-outcome">
                          {outcome(item.value.latest_execution)}
                        </span>
                      </td>
                      <td class="next-run">
                        <Show when={"enabled" in item.value} fallback="—">
                          <span
                            title={
                              "next_run_at_ms" in item.value &&
                              item.value.next_run_at_ms !== null
                                ? new Date(
                                    item.value.next_run_at_ms,
                                  ).toLocaleString()
                                : undefined
                            }
                          >
                            {"enabled" in item.value &&
                              (item.value.enabled
                                ? when(item.value.next_run_at_ms)
                                : "Paused")}
                          </span>
                        </Show>
                      </td>
                      <td>
                        <a
                          class="icon-button row-open"
                          aria-label={`Inspect ${item.value.id}`}
                          href={`#/${item.kind}s/${encodeURIComponent(item.value.id)}`}
                        >
                          <Icon name="chevron" />
                        </a>
                      </td>
                    </tr>
                  );
                }}
              </For>
            </tbody>
          </table>
        </div>
      </Show>
    </section>
  );
}
