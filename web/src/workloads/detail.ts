import type { Api } from "../api/client";
import {
  serviceDetailSchema,
  scheduleDetailSchema,
  historySchema,
  scheduleHistorySchema,
  type Service,
  type Schedule,
  type ServiceDefinition,
  type ScheduleDefinition,
  type Execution,
} from "../api/contracts";
import type { Kind } from "./presentation";
export type Detail =
  | {
      kind: "service";
      value: Service;
      definition: ServiceDefinition;
      history: Execution[];
    }
  | {
      kind: "schedule";
      value: Schedule;
      definition: ScheduleDefinition;
      history: Execution[];
    };
export async function loadDetail(
  api: Api,
  kind: Kind,
  id: string,
): Promise<Detail> {
  if (kind === "service") {
    const [detail, history] = await Promise.all([
      api({ operation: "inspect", id }, serviceDetailSchema),
      api({ operation: "history", id }, historySchema),
    ]);
    return {
      kind,
      value: detail.service,
      definition: detail.definition,
      history: history.executions,
    };
  }
  const [detail, history] = await Promise.all([
    api(
      { operation: "schedule", command: { operation: "inspect", id } },
      scheduleDetailSchema,
    ),
    api(
      { operation: "schedule", command: { operation: "history", id } },
      scheduleHistorySchema,
    ),
  ]);
  return {
    kind,
    value: detail.response.schedule,
    definition: detail.response.definition,
    history: history.response.executions,
  };
}
