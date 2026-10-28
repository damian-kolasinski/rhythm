import { z } from "zod";

export const outcomeSchema = z.discriminatedUnion("type", [
  z.object({ type: z.literal("exited"), code: z.number().int() }),
  z.object({ type: z.literal("signalled"), signal: z.number().int() }),
  ...[
    "stopped",
    "interrupted",
    "spawn_failed",
    "monitor_lost",
    "output_unavailable",
  ].map((type) => z.object({ type: z.literal(type) })),
]);
export const executionSchema = z.object({
  id: z.number().int().positive(),
  started_at_ms: z.number(),
  finished_at_ms: z.number().nullable(),
  outcome: outcomeSchema.nullable(),
  recovery_acknowledged_at_ms: z.number().nullable(),
});
export type Execution = z.infer<typeof executionSchema>;
export const observedSchema = z.object({
  state: z.enum([
    "stopped",
    "running",
    "recovering",
    "waiting",
    "exited",
    "needs_attention",
    "idle",
  ]),
  execution_id: z.number().int().optional(),
  retry_at_ms: z.number().optional(),
});
export const invocationSchema = z.union([
  z.object({ source: z.literal("manual"), requested_at_ms: z.number() }),
  z.object({ source: z.literal("scheduled"), scheduled_for_ms: z.number() }),
]);
export const serviceSchema = z.object({
  id: z.string(),
  desired: z.enum(["running", "stopped"]),
  observed: observedSchema,
  latest_execution: executionSchema.nullable(),
});
export const scheduleSchema = z.object({
  id: z.string(),
  enabled: z.boolean(),
  observed: observedSchema,
  latest_execution: executionSchema.nullable(),
  next_run_at_ms: z.number().nullable(),
  pending: invocationSchema.nullable(),
  last_decision: z
    .object({
      scheduled_for_ms: z.number(),
      decided_at_ms: z.number(),
      action: z.enum(["queued", "skipped_missed", "skipped_overlap"]),
    })
    .nullable(),
});
export type Service = z.infer<typeof serviceSchema>;
export type Schedule = z.infer<typeof scheduleSchema>;
export const triggerSchema = z.discriminatedUnion("type", [
  z.object({ type: z.literal("interval"), every_ms: z.number() }),
  z.object({
    type: z.literal("daily"),
    hour: z.number(),
    minute: z.number(),
    timezone: z.string(),
  }),
  z.object({
    type: z.literal("weekly"),
    weekdays: z.array(z.number()),
    hour: z.number(),
    minute: z.number(),
    timezone: z.string(),
  }),
]);
export type Trigger = z.infer<typeof triggerSchema>;
const command = {
  id: z.string(),
  command: z.array(z.string()),
  working_directory: z.string(),
};
export const serviceDefinitionSchema = z.object({
  ...command,
  restart: z.enum(["never", "on_failure", "always"]),
  restart_delay_ms: z.number(),
});
export const scheduleDefinitionSchema = z.object({
  ...command,
  trigger: triggerSchema,
  overlap: z.enum(["skip", "queue_one"]),
  missed: z.enum(["skip", "run_once"]),
  grace_ms: z.number(),
});
export type ServiceDefinition = z.infer<typeof serviceDefinitionSchema>;
export type ScheduleDefinition = z.infer<typeof scheduleDefinitionSchema>;
export const serviceListSchema = z.object({
  type: z.literal("services"),
  services: z.array(serviceSchema),
});
export const scheduleListSchema = z.object({
  type: z.literal("schedule"),
  response: z.object({
    type: z.literal("schedules"),
    schedules: z.array(scheduleSchema),
  }),
});
export const serviceDetailSchema = z.object({
  type: z.literal("service"),
  service: serviceSchema,
  definition: serviceDefinitionSchema,
});
export const scheduleDetailSchema = z.object({
  type: z.literal("schedule"),
  response: z.object({
    type: z.literal("schedule"),
    schedule: scheduleSchema,
    definition: scheduleDefinitionSchema,
  }),
});
export const historySchema = z.object({
  type: z.literal("history"),
  executions: z.array(executionSchema),
});
export const scheduleHistorySchema = z.object({
  type: z.literal("schedule"),
  response: historySchema,
});
export const logsSchema = z.object({
  type: z.literal("logs"),
  stdout: z.string(),
  stderr: z.string(),
});
export const statusSchema = z.object({
  type: z.literal("runtime"),
  version: z.string(),
});
export const workspaceInfoSchema = z.object({ demo: z.boolean() });
