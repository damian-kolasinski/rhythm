import { For, Show } from "solid-js";
import { status, outcome, when, type Item } from "../workloads/presentation";
import { Icon } from "./Icon";
export function WorkTable(props: {
  title: string;
  subtitle: string;
  items: Item[];
  empty: string;
  newWork?: () => void;
  unavailable?: boolean;
}) {
  return (
    <section class="work-section">
      <div class="section-heading">
        <div>
          <h2>
            {props.title}
            <span class="section-count">{props.items.length}</span>
          </h2>
          <p>{props.subtitle}</p>
        </div>
        <Show when={props.newWork}>
          <button
            class="text-button"
            onClick={props.newWork}
            disabled={props.unavailable}
          >
            <Icon name="plus" />
            Add {props.title === "Services" ? "service" : "schedule"}
          </button>
        </Show>
      </div>
      <Show
        when={props.items.length}
        fallback={
          <div class="empty-section">
            <Icon name={props.title === "Services" ? "service" : "clock"} />
            <span>{props.empty}</span>
          </div>
        }
      >
        <div class="table-scroll">
          <table>
            <thead>
              <tr>
                <th>Name</th>
                <th>Status</th>
                <th>Latest execution</th>
                <th>
                  {props.title === "Schedules"
                    ? "Next automatic run"
                    : "Desired state"}
                </th>
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
                          href={`#/${item.kind === "service" ? "services" : "schedules"}/${encodeURIComponent(item.value.id)}`}
                        >
                          <span class={`work-icon ${item.kind}`}>
                            <Icon
                              name={
                                item.kind === "service" ? "service" : "clock"
                              }
                            />
                          </span>
                          <span>
                            {item.value.id}
                            <small>Rhythm-owned {item.kind}</small>
                          </span>
                        </a>
                      </td>
                      <td>
                        <span class={`status ${state().tone}`}>
                          <i />
                          {state().label}
                        </span>
                      </td>
                      <td>
                        <span class="table-outcome">
                          {outcome(item.value.latest_execution)}
                        </span>
                        <small class="cell-subtitle">
                          {item.value.latest_execution
                            ? `#${item.value.latest_execution!.id}`
                            : "History starts with the first run"}
                        </small>
                      </td>
                      <td class="table-secondary">
                        {"enabled" in item.value
                          ? item.value.enabled
                            ? when(item.value.next_run_at_ms)
                            : "Paused"
                          : item.value.desired === "running"
                            ? "Running"
                            : "Stopped"}
                      </td>
                      <td>
                        <a
                          class="icon-button row-open"
                          aria-label={`Inspect ${item.value.id}`}
                          href={`#/${item.kind === "service" ? "services" : "schedules"}/${encodeURIComponent(item.value.id)}`}
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
