import { For, Show } from "solid-js";
import { status, outcome, when } from "../schedules/presentation";
import type { Schedule } from "../api/contracts";
import { detailHash } from "../schedules/navigation";
import { Icon } from "./Icon";

export function ScheduleTable(props: {
  schedules: Schedule[];
  filtered: boolean;
}) {
  return (
    <section class="work-section" aria-label="Schedule list">
      <Show
        when={props.schedules.length}
        fallback={
          <div class="empty-section">
            <p>
              {props.filtered
                ? "No schedules match these filters."
                : "No schedules yet."}
            </p>
            <Show when={!props.filtered}>
              <p class="subtle">
                Create a schedule with the CLI:{" "}
                <code>rhythm schedule create --file FILE</code>.
              </p>
            </Show>
          </div>
        }
      >
        <div class="table-scroll">
          <table aria-label="Schedules">
            <thead>
              <tr>
                <th>Name</th>
                <th>Status</th>
                <th class="latest-execution">Latest result</th>
                <th class="next-run">Next run</th>
                <th>
                  <span class="sr-only">Open details</span>
                </th>
              </tr>
            </thead>
            <tbody>
              <For each={props.schedules}>
                {(schedule) => {
                  const state = () => status(schedule);
                  return (
                    <tr>
                      <td>
                        <a
                          class="work-name"
                          href={`#${detailHash(schedule.id)}`}
                        >
                          {schedule.id}
                        </a>
                      </td>
                      <td>
                        <span class={`status ${state().tone}`}>
                          <i />
                          {state().label}
                        </span>
                        <Show when={schedule.enabled}>
                          <small class="compact-next">
                            Next {when(schedule.next_run_at_ms)}
                          </small>
                        </Show>
                      </td>
                      <td class="latest-execution">
                        <span class="table-outcome">
                          {outcome(schedule.latest_execution)}
                        </span>
                      </td>
                      <td class="next-run">
                        <span
                          title={
                            schedule.next_run_at_ms !== null
                              ? new Date(
                                  schedule.next_run_at_ms,
                                ).toLocaleString()
                              : undefined
                          }
                        >
                          {schedule.enabled
                            ? when(schedule.next_run_at_ms)
                            : "Paused"}
                        </span>
                      </td>
                      <td>
                        <a
                          class="icon-button row-open"
                          aria-label={`Inspect ${schedule.id}`}
                          href={`#${detailHash(schedule.id)}`}
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
