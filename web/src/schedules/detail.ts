import type { Api } from "../api/client";
import {
  scheduleDetailSchema,
  historySchema,
  type Schedule,
  type ScheduleDefinition,
  type Execution,
} from "../api/contracts";
export type Detail = {
  schedule: Schedule;
  definition: ScheduleDefinition;
  history: Execution[];
};
export async function loadDetail(api: Api, id: string): Promise<Detail> {
  const [detail, history] = await Promise.all([
    api({ operation: "inspect", id }, scheduleDetailSchema),
    api({ operation: "history", id }, historySchema),
  ]);
  return {
    schedule: detail.schedule,
    definition: detail.definition,
    history: history.executions,
  };
}
