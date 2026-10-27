import { test, expect, type Route } from "@playwright/test";
import { Runtime } from "./runtime";
import { cadence, parseArguments, when } from "../src/workloads/presentation";
const now = Date.parse("2026-01-15T09:00:00Z");
const run = (id: number, outcome: any) => ({
  id,
  started_at_ms: now - 60000,
  finished_at_ms: outcome ? now - 30000 : null,
  outcome,
  recovery_acknowledged_at_ms: null,
});
const service = (id: string, state: string, outcome: any, execution = 1) => ({
  id,
  desired: state === "stopped" ? "stopped" : "running",
  observed: {
    state,
    ...(state === "running" || state === "recovering"
      ? { execution_id: execution }
      : {}),
  },
  latest_execution: state === "stopped" ? null : run(execution, outcome),
});
const schedule = (
  id: string,
  enabled: boolean,
  state = "idle",
  outcome: any = null,
  execution = 10,
) => ({
  id,
  enabled,
  observed: { state },
  latest_execution: outcome ? run(execution, outcome) : null,
  next_run_at_ms: enabled ? now + 5400000 : null,
  pending: null,
  last_decision: null,
});
const standard = {
  services: [
    service("sample-indexer", "running", null),
    service("sample-export", "exited", { type: "exited", code: 4 }, 2),
    service("sample-notifier", "stopped", null),
  ],
  schedules: [
    schedule("sample-report", true),
    schedule("sample-backup", false),
  ],
};
let runtime: Runtime;
test.beforeEach(async () => {
  runtime = await Runtime.start();
});
test.afterEach(async () => {
  await runtime.stop();
});

async function mock(
  route: Route,
  services: any[],
  schedules: any[],
  error = false,
) {
  const body = route.request().postDataJSON();
  if (error)
    return route.fulfill({
      status: 503,
      json: { error: { code: "runtime_unavailable" } },
    });
  if (body.operation === "status")
    return route.fulfill({ json: { type: "runtime", version: "0.1.0" } });
  if (body.operation === "list")
    return route.fulfill({ json: { type: "services", services } });
  if (body.operation === "schedule" && body.command.operation === "list")
    return route.fulfill({
      json: { type: "schedule", response: { type: "schedules", schedules } },
    });
  if (body.operation === "inspect")
    return route.fulfill({
      json: {
        type: "service",
        service: services.find((s) => s.id === body.id),
        definition: {
          id: body.id,
          command: ["/bin/sh", "-c", "printf sample-output"],
          working_directory: "/tmp",
          restart: "never",
          restart_delay_ms: 1000,
        },
      },
    });
  if (body.operation === "history")
    return route.fulfill({
      json: {
        type: "history",
        executions: [
          services.find((s) => s.id === body.id)?.latest_execution,
        ].filter(Boolean),
      },
    });
  if (body.operation === "logs")
    return route.fulfill({
      json: {
        type: "logs",
        stdout:
          "<task> sample output </task>\nWork is retained across sessions.\n",
        stderr: "Sample configuration needs a closer look.\n",
      },
    });
  return route.fulfill({
    status: 400,
    json: { error: { code: "invalid_request" } },
  });
}
for (const state of [
  "empty",
  "populated",
  "attention",
  "recovering",
  "error",
  "unsupported-response",
] as const) {
  test(`render ${state}`, async ({ page, context }) => {
    await runtime.session(context);
    await page.addInitScript((value) => {
      Date.now = () => value;
    }, now);
    const services =
      state === "empty"
        ? []
        : state === "attention"
          ? [
              service("sample-uncertain", "needs_attention", {
                type: "monitor_lost",
              }),
            ]
          : state === "recovering"
            ? [service("sample-recovering", "recovering", null)]
            : state === "unsupported-response"
              ? [service("sample-unknown", "unknown", null)]
              : standard.services;
    const schedules =
      state === "empty"
        ? []
        : state === "attention"
          ? [
              schedule("sample-output", true, "needs_attention", {
                type: "output_unavailable",
              }),
            ]
          : standard.schedules;
    await page.route("**/v1/browser/command", (r) =>
      mock(r, services, schedules, state === "error"),
    );
    await page.goto(runtime.origin);
    if (state === "error" || state === "unsupported-response")
      await expect(page.getByRole("alert")).toBeVisible();
    else
      await expect(
        page.getByRole("heading", { name: "Services", exact: false }).last(),
      ).toBeVisible();
    await expect(page).toHaveScreenshot(`${state}.png`, { fullPage: true });
  });
}
test("render loading", async ({ page, context }) => {
  const errors: string[] = [];
  page.on("pageerror", (e) => errors.push(e.message));
  await runtime.session(context);
  const requests: Route[] = [];
  await page.route("**/v1/browser/command", (route) => {
    requests.push(route);
  });
  await page.goto(runtime.origin);
  await expect(page.getByText("Loading your workspace…")).toBeVisible();
  await page.keyboard.press("Control+k");
  await page.keyboard.press("Meta+k");
  expect(errors).toEqual([]);
  await expect(page).toHaveScreenshot("loading.png", { fullPage: true });
  for (const request of requests) await request.abort();
});
test("render login and mobile overview", async ({ page, context }) => {
  await page.goto(runtime.origin);
  await expect(
    page.getByRole("heading", { name: "Good work keeps going." }),
  ).toBeVisible();
  await expect(page).toHaveScreenshot("login.png");
  await runtime.session(context);
  await page.addInitScript((value) => {
    Date.now = () => value;
  }, now);
  await page.route("**/v1/browser/command", (r) =>
    mock(r, standard.services, standard.schedules),
  );
  await page.setViewportSize({ width: 390, height: 844 });
  await page.reload();
  await expect(
    page.getByRole("link", { name: "Inspect sample-indexer" }),
  ).toBeVisible();
  await expect(page).toHaveScreenshot("mobile.png", { fullPage: true });
});
test("render uncertain cleanup and literal execution output", async ({
  page,
  context,
}) => {
  await runtime.session(context);
  await page.addInitScript((value) => {
    Date.now = () => value;
  }, now);
  const services = [
    service("sample-uncertain", "needs_attention", { type: "monitor_lost" }),
  ];
  await page.route("**/v1/browser/command", (r) => mock(r, services, []));
  await page.goto(`${runtime.origin}/#/services/sample-uncertain`);
  const dialog = page.getByRole("dialog", {
    name: "sample-uncertain",
    exact: true,
  });
  await expect(dialog.getByText("Completion is uncertain")).toBeVisible();
  await dialog
    .getByRole("button", { name: "Acknowledge cleanup", exact: true })
    .click();
  const confirm = page.getByRole("dialog", { name: "Acknowledge cleanup?" });
  await expect(
    confirm.getByRole("button", { name: "Acknowledge cleanup", exact: true }),
  ).toBeDisabled();
  await expect(page).toHaveScreenshot("cleanup.png");
  await confirm.getByRole("button", { name: "Keep as is" }).click();
  await dialog
    .getByRole("button", { name: "View logs for execution 1" })
    .click();
  await expect(dialog.getByTestId("stdout")).toContainText(
    "<task> sample output </task>",
  );
  await expect(page).toHaveScreenshot("logs.png");
});
test("argument and recurrence presentation boundaries", () => {
  expect(parseArguments("")).toEqual([]);
  expect(parseArguments("one\r\n\r\n two ")).toEqual(["one", "", " two "]);
  expect(parseArguments('$(literal)\n"quoted"')).toEqual([
    "$(literal)",
    '"quoted"',
  ]);
  expect(
    cadence({
      type: "weekly",
      weekdays: [1, 5],
      hour: 9,
      minute: 5,
      timezone: "UTC",
    }),
  ).toBe("Mon, Fri at 09:05 · UTC");
  expect(when(now + 10000, now)).toBe("in 10s");
  expect(when(null, now)).toBe("—");
  expect(when(now, now)).toBe("Due now");
});

test("render service and schedule creation forms and mobile detail", async ({
  page,
  context,
}) => {
  await runtime.session(context);
  await page.addInitScript((value) => {
    Date.now = () => value;
  }, now);
  await page.route("**/v1/browser/command", (r) =>
    mock(r, standard.services, standard.schedules),
  );
  await page.goto(runtime.origin);
  await expect(
    page.getByRole("link", { name: "Inspect sample-indexer" }),
  ).toBeVisible();
  await page.getByRole("button", { name: "New service", exact: true }).click();
  await expect(page.getByRole("dialog", { name: "New service" })).toBeVisible();
  await expect(page).toHaveScreenshot("create-service.png");
  await page.keyboard.press("Escape");
  await expect(page.getByRole("dialog")).toHaveCount(0);
  await page.getByRole("button", { name: "New schedule", exact: true }).click();
  const form = page.getByRole("dialog", { name: "New schedule" });
  await expect(form).toBeVisible();
  await expect(page).toHaveScreenshot("create-interval.png");
  await form.getByLabel("Trigger", { exact: true }).selectOption("weekly");
  await expect(page).toHaveScreenshot("create-weekly.png");
  await form.getByLabel("Trigger", { exact: true }).selectOption("daily");
  await expect(page).toHaveScreenshot("create-daily.png");
  await page.keyboard.press("Escape");
  await expect(form).toHaveCount(0);
  await page.setViewportSize({ width: 390, height: 844 });
  await page.getByRole("link", { name: "Inspect sample-indexer" }).click();
  await expect(page.getByRole("dialog").locator(".status")).toHaveText(
    "Running",
  );
  await expect(page).toHaveScreenshot("mobile-detail.png");
});
