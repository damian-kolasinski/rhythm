import { test, expect, type Route } from "@playwright/test";
import { Runtime } from "./runtime";
import { cadence, when } from "../src/schedules/presentation";
const now = Date.parse("2026-01-15T09:00:00Z");
const run = (id: number, owner: string, outcome: any) => ({
  id,
  schedule_id: owner,
  invocation: { source: "manual", requested_at_ms: now - 60000 },
  started_at_ms: now - 60000,
  finished_at_ms: outcome ? now - 30000 : null,
  outcome,
  recovery_acknowledged_at_ms: null,
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
  latest_execution:
    outcome || ["running", "recovering"].includes(state)
      ? run(execution, id, outcome)
      : null,
  next_run_at_ms: enabled && state !== "needs_attention" ? now + 5400000 : null,
  pending: null,
  last_decision: null,
});
const standard = [
  schedule("sample-report", true, "running"),
  schedule("sample-export", false, "idle", { type: "exited", code: 4 }, 2),
  schedule("sample-backup", false),
  schedule("sample-cleanup", true),
];
let runtime: Runtime;
test.beforeEach(async () => {
  runtime = await Runtime.start();
});
test.afterEach(async () => {
  await runtime.stop();
});

async function mock(route: Route, schedules: any[], error = false) {
  const body = route.request().postDataJSON();
  if (error)
    return route.fulfill({
      status: 503,
      json: { error: { code: "runtime_unavailable" } },
    });
  if (body.operation === "status")
    return route.fulfill({ json: { type: "runtime", version: "0.1.1" } });
  if (body.operation === "list")
    return route.fulfill({ json: { type: "schedules", schedules } });
  if (body.operation === "inspect")
    return route.fulfill({
      json: {
        type: "schedule",
        schedule: schedules.find((s) => s.id === body.id),
        definition: {
          id: body.id,
          command: ["/bin/sh", "-c", "printf sample-output"],
          working_directory: "/tmp",
          trigger: { type: "interval", every_ms: 60000 },
          overlap: "skip",
          missed: "run_once",
          grace_ms: 1000,
        },
      },
    });
  if (body.operation === "history")
    return route.fulfill({
      json: {
        type: "history",
        executions: [
          schedules.find((s) => s.id === body.id)?.latest_execution,
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
    await page.addInitScript((value) => {
      Date.now = () => value;
    }, now);
    const schedules =
      state === "empty"
        ? []
        : state === "attention"
          ? [
              schedule("sample-uncertain", true, "needs_attention", {
                type: "monitor_lost",
              }),
              schedule("sample-output", true, "needs_attention", {
                type: "output_unavailable",
              }),
            ]
          : state === "recovering"
            ? [schedule("sample-recovering", true, "recovering")]
            : state === "unsupported-response"
              ? [schedule("sample-unknown", true, "unknown")]
              : standard;
    await page.route("**/v1/browser/command", (r) =>
      mock(r, schedules, state === "error"),
    );
    await page.goto(runtime.origin);
    if (state === "error" || state === "unsupported-response")
      await expect(page.getByRole("alert")).toBeVisible();
    else
      await expect(
        page.getByRole("region", { name: "Schedule list", exact: true }),
      ).toBeVisible();
    await expect(page).toHaveScreenshot(`${state}.png`, { fullPage: true });
  });
}
test("render loading", async ({ page, context }) => {
  const errors: string[] = [];
  page.on("pageerror", (e) => errors.push(e.message));
  const requests: Route[] = [];
  await page.route("**/v1/browser/command", (route) => {
    requests.push(route);
  });
  await page.goto(runtime.origin);
  await expect(page.getByText("Loading schedules…")).toBeVisible();
  await page.keyboard.press("Control+k");
  await page.keyboard.press("Meta+k");
  expect(errors).toEqual([]);
  await expect(page).toHaveScreenshot("loading.png", { fullPage: true });
  for (const request of requests) await request.abort();
});
test("render mobile list", async ({ page }) => {
  await page.addInitScript((value) => {
    Date.now = () => value;
  }, now);
  await page.route("**/v1/browser/command", (r) => mock(r, standard));
  await page.setViewportSize({ width: 390, height: 844 });
  await page.goto(runtime.origin);
  await expect(
    page.getByRole("link", { name: "Inspect sample-report" }),
  ).toBeVisible();
  await expect(page).toHaveScreenshot("mobile.png", { fullPage: true });
});
test("render no matching work", async ({ page }) => {
  await page.route("**/v1/browser/command", (r) => mock(r, standard));
  await page.goto(runtime.origin);
  await page
    .getByRole("searchbox", { name: "Search schedules" })
    .fill("missing");
  await expect(
    page.getByText("No schedules match these filters."),
  ).toBeVisible();
  await expect(page).toHaveScreenshot("no-matches.png", { fullPage: true });
});

test("render narrow list with long names and upcoming timing", async ({
  page,
}) => {
  await page.addInitScript((value) => {
    Date.now = () => value;
  }, now);
  const longName = "sample-" + "a".repeat(56);
  await page.route("**/v1/browser/command", (r) =>
    mock(r, [...standard, schedule(longName, false)]),
  );
  await page.setViewportSize({ width: 712, height: 900 });
  await page.goto(runtime.origin);
  await expect(
    page.getByRole("link", { name: `Inspect ${longName}` }),
  ).toBeVisible();
  await expect(
    page.getByRole("columnheader", { name: "Next run", exact: true }),
  ).toBeVisible();
  await expect(
    page.getByRole("table", { name: "Schedules", exact: true }),
  ).toContainText("in 1h 30m");
  expect(
    await page.evaluate(
      () => document.documentElement.scrollWidth <= innerWidth,
    ),
  ).toBe(true);
  await expect(page).toHaveScreenshot("narrow.png", { fullPage: true });
});

test("render uncertain cleanup and literal execution output", async ({
  page,
  context,
}) => {
  await page.addInitScript((value) => {
    Date.now = () => value;
  }, now);
  const schedules = [
    schedule(
      "sample-uncertain",
      true,
      "needs_attention",
      { type: "monitor_lost" },
      1,
    ),
  ];
  await page.route("**/v1/browser/command", (r) => mock(r, schedules));
  await page.goto(`${runtime.origin}/#/sample-uncertain`);
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
test("recurrence presentation boundaries", () => {
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

test("render mobile detail", async ({ page, context }) => {
  await page.addInitScript((value) => {
    Date.now = () => value;
  }, now);
  await page.route("**/v1/browser/command", (r) => mock(r, standard));
  await page.goto(runtime.origin);
  await expect(
    page.getByRole("link", { name: "Inspect sample-report" }),
  ).toBeVisible();
  await page.setViewportSize({ width: 390, height: 844 });
  await page.getByRole("link", { name: "Inspect sample-report" }).click();
  await expect(page.getByRole("dialog").locator(".status")).toHaveText(
    "Running",
  );
  await expect(page).toHaveScreenshot("mobile-detail.png");
});

for (const theme of ["light", "dark"] as const) {
  test(`mergegate appearance ${theme}`, async ({ page }) => {
    await page.emulateMedia({ colorScheme: theme });
    await page.addInitScript((value) => {
      Date.now = () => value;
    }, now);
    await page.route("**/v1/browser/command", (r) => mock(r, standard));
    await page.goto(runtime.origin);
    await expect(
      page.getByRole("link", { name: "Inspect sample-report" }),
    ).toBeVisible();
    await page.evaluate(() => document.fonts.ready);
    expect(
      await page.evaluate(() => document.fonts.check('14px "DM Sans"')),
    ).toBe(true);
    const actual = await page.evaluate(() => ({
      face: getComputedStyle(document.body).fontFamily,
      page: getComputedStyle(document.documentElement).backgroundColor,
      brand: getComputedStyle(document.querySelector(".brand svg")!).color,
    }));
    expect(actual.face).toContain("DM Sans");
    expect(actual.page).toBe(
      theme === "light" ? "rgb(247, 252, 255)" : "rgb(34, 37, 38)",
    );
    expect(actual.brand).toBe(
      theme === "light" ? "rgb(0, 59, 102)" : "rgb(229, 244, 255)",
    );
    await expect(page).toHaveScreenshot(`appearance-${theme}.png`, {
      fullPage: true,
    });
    if (theme === "dark") {
      await page.getByRole("link", { name: "Inspect sample-export" }).click();
      await expect(page.getByRole("dialog").locator(".status")).toHaveText(
        "Failed",
      );
      await page
        .getByRole("dialog")
        .getByRole("button", { name: /View logs/ })
        .first()
        .click();
      await expect(page.getByTestId("stdout")).toBeVisible();
      expect(
        await page
          .getByTestId("stdout")
          .evaluate((el) => getComputedStyle(el).fontFamily),
      ).toContain("monospace");
      await expect(page).toHaveScreenshot("dark-detail.png");
      await page
        .getByRole("dialog")
        .getByRole("tab", { name: "Definition", exact: true })
        .click();
      expect(
        await page
          .locator(".definition-panel code")
          .first()
          .evaluate((el) => getComputedStyle(el).fontFamily),
      ).toContain("monospace");
    }
  });
}
