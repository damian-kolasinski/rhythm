import { createServer } from "node:http";
import { readFile } from "node:fs/promises";
import { join } from "node:path";
import { test, expect } from "@playwright/test";
import { Runtime, service, schedule, waitForValue } from "./runtime";
let runtime: Runtime;
test.beforeEach(async () => {
  runtime = await Runtime.start();
});
test.afterEach(async () => {
  await runtime.stop();
});

test("open the CLI URL directly, reload, and open another tab without pairing", async ({
  page,
  browser,
}) => {
  const launch = await runtime.launch();
  expect(Object.keys(launch).sort()).toEqual(["demo", "url"]);
  const credentials: string[] = [];
  page.on("request", (r) => {
    if (r.headers().authorization) credentials.push(r.headers().authorization);
  });
  await page.goto(launch.url);
  await expect(
    page.getByRole("heading", { name: "Work", exact: true }),
  ).toBeVisible();
  await expect(
    page.getByText("No work registered yet.", { exact: true }),
  ).toBeVisible();
  expect(
    await page.evaluate(() => ({
      session: sessionStorage.length,
      local: localStorage.length,
    })),
  ).toEqual({ session: 0, local: 0 });
  expect(await page.context().cookies()).toEqual([]);
  expect(credentials).toEqual([]);
  await page.reload();
  await expect(
    page.getByText("No work registered yet.", { exact: true }),
  ).toBeVisible();
  const context = await browser.newContext();
  try {
    const second = await context.newPage();
    await second.goto(launch.url);
    await expect(
      second.getByText("No work registered yet.", { exact: true }),
    ).toBeVisible();
  } finally {
    await context.close();
  }
  const refused = await page.evaluate(async () => {
    const r = await fetch("/v1/browser/command", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ operation: "list" }),
    });
    return r.status;
  });
  expect(refused).toBe(401);
});

test("CLI-created service can start, show literal logs, restart, and stop", async ({
  page,
}) => {
  const errors: string[] = [];
  page.on("pageerror", (e) => errors.push(e.message));
  await runtime.create(
    "service",
    service(
      "sample-ui-service",
      'printf "<img src=x onerror=alert(1)>"; exec sleep 30',
    ),
  );
  await runtime.connect(page);
  await expect(
    page.getByRole("button", { name: /New service|Add service/ }),
  ).toHaveCount(0);
  await page.getByRole("link", { name: "Inspect sample-ui-service" }).click();
  const detail = page.getByRole("dialog", {
    name: "sample-ui-service",
    exact: true,
  });
  await expect(
    detail.getByText("Stopped", { exact: true }).first(),
  ).toBeVisible();
  await detail.getByRole("button", { name: "Start", exact: true }).click();
  await expect(detail.locator(".status")).toHaveText("Running");
  await detail
    .getByRole("button", { name: /View logs for execution/ })
    .first()
    .click();
  await expect(detail.getByTestId("stdout")).toHaveText(
    "<img src=x onerror=alert(1)>",
  );
  await expect(detail.getByTestId("stdout").locator("img")).toHaveCount(0);
  const before = await runtime.command({
    operation: "history",
    id: "sample-ui-service",
  });
  await detail.getByRole("button", { name: "Restart", exact: true }).click();
  let confirm = page.getByRole("dialog", { name: "Restart this service?" });
  await confirm.getByRole("button", { name: "Keep as is" }).click();
  expect(
    (await runtime.command({ operation: "history", id: "sample-ui-service" }))
      .executions.length,
  ).toBe(before.executions.length);
  await detail.getByRole("button", { name: "Restart", exact: true }).click();
  confirm = page.getByRole("dialog", { name: "Restart this service?" });
  await confirm.getByRole("button", { name: "Restart", exact: true }).click();
  await waitForValue(
    () => runtime.command({ operation: "history", id: "sample-ui-service" }),
    (v) => v.executions.length === 2,
  );
  await detail.getByRole("button", { name: "Stop", exact: true }).click();
  await page
    .getByRole("dialog", { name: "Stop this service?" })
    .getByRole("button", { name: "Stop", exact: true })
    .click();
  await expect(detail.locator(".status")).toHaveText("Stopped");
  expect(errors).toEqual([]);
});

test("CLI-created schedule can run now, show history, resume, pause, and cancel queued work", async ({
  page,
}) => {
  await runtime.create(
    "schedule",
    schedule(
      "sample-ui-schedule",
      "printf schedule-output; exec sleep 30",
      500,
    ),
  );
  await runtime.connect(page);
  await page.getByRole("link", { name: "Inspect sample-ui-schedule" }).click();
  const detail = page.getByRole("dialog", {
    name: "sample-ui-schedule",
    exact: true,
  });
  await expect(detail.locator(".status")).toHaveText("Paused");
  await detail.getByRole("button", { name: "Run now", exact: true }).click();
  await expect(detail.locator(".status")).toHaveText("Running");
  await detail
    .getByRole("button", { name: /View logs for execution/ })
    .first()
    .click();
  await expect(detail.getByTestId("stdout")).toHaveText("schedule-output");
  await detail.getByRole("button", { name: "Resume", exact: true }).click();
  await expect(
    detail.getByRole("button", { name: "Pause", exact: true }),
  ).toBeVisible();
  await detail.getByRole("button", { name: "Pause", exact: true }).click();
  await page
    .getByRole("dialog", { name: "Pause this schedule?" })
    .getByRole("button", { name: "Pause", exact: true })
    .click();
  await expect(
    detail.getByRole("button", { name: "Resume", exact: true }),
  ).toBeVisible();
  await detail.getByRole("button", { name: "Cancel run", exact: true }).click();
  await page
    .getByRole("dialog", { name: "Cancel active and queued work?" })
    .getByRole("button", { name: "Cancel run", exact: true })
    .click();
  await expect(detail.locator(".status")).toHaveText("Paused");
  const stored = await runtime.command({
    operation: "schedule",
    command: { operation: "inspect", id: "sample-ui-schedule" },
  });
  expect(stored.response.schedule.enabled).toBe(false);
  expect(stored.response.schedule.pending).toBeNull();
});

test("CLI calendar definitions, combined filters, keyboard focus, and responsive detail", async ({
  page,
}) => {
  await runtime.command({
    operation: "create",
    definition: service("sample-find"),
  });
  await runtime.command({
    operation: "schedule",
    command: { operation: "create", definition: schedule("sample-hidden") },
  });
  await runtime.create("schedule", {
    ...schedule("sample-weekly"),
    trigger: {
      type: "weekly",
      weekdays: [1, 5],
      hour: 9,
      minute: 15,
      timezone: "UTC",
    },
  });
  await runtime.create("schedule", {
    ...schedule("sample-daily"),
    trigger: { type: "daily", hour: 18, minute: 30, timezone: "UTC" },
  });
  await runtime.connect(page);
  await page
    .getByRole("combobox", { name: "Filter kind" })
    .selectOption("schedule");
  await expect(
    page.getByRole("link", { name: "Inspect sample-find" }),
  ).toHaveCount(0);
  await page.getByRole("link", { name: "Inspect sample-weekly" }).click();
  let detail = page.getByRole("dialog", { name: "sample-weekly", exact: true });
  await detail.getByRole("tab", { name: "Activity", exact: true }).focus();
  await page.keyboard.press("ArrowRight");
  await expect(
    detail.getByRole("tab", { name: "Definition", exact: true }),
  ).toBeFocused();
  await expect(
    detail.getByRole("tabpanel", { name: "Definition", exact: true }),
  ).toBeVisible();
  await page.keyboard.press("Home");
  await expect(
    detail.getByRole("tab", { name: "Activity", exact: true }),
  ).toBeFocused();
  await page.keyboard.press("End");
  await expect(
    detail.getByRole("tab", { name: "Definition", exact: true }),
  ).toBeFocused();
  await expect(detail.getByText("Mon, Fri at 09:15 · UTC")).toBeVisible();
  await page.keyboard.press("Escape");
  await expect(detail).toHaveCount(0);
  await expect(page.getByRole("combobox", { name: "Filter kind" })).toHaveValue(
    "schedule",
  );
  await page.getByRole("link", { name: "Inspect sample-daily" }).click();
  detail = page.getByRole("dialog", { name: "sample-daily", exact: true });
  await detail.getByRole("tab", { name: "Definition", exact: true }).click();
  await expect(detail.getByText("Daily at 18:30 · UTC")).toBeVisible();
  await page.keyboard.press("Escape");
  await expect(detail).toHaveCount(0);
  await page.getByRole("combobox", { name: "Filter kind" }).selectOption("all");
  await page.getByRole("searchbox", { name: "Search work" }).fill("find");
  await expect(
    page.getByRole("link", { name: "Inspect sample-find" }),
  ).toBeVisible();
  await expect(
    page.getByRole("link", { name: "Inspect sample-hidden" }),
  ).toHaveCount(0);
  await page.getByRole("searchbox", { name: "Search work" }).fill("");
  await page.getByRole("link", { name: "Inspect sample-find" }).focus();
  await page.waitForTimeout(2400);
  await expect(
    page.getByRole("link", { name: "Inspect sample-find" }),
  ).toBeFocused();
  await page.setViewportSize({ width: 390, height: 844 });
  await page.getByRole("link", { name: "Inspect sample-find" }).click();
  detail = page.getByRole("dialog", { name: "sample-find", exact: true });
  await expect(detail).toBeVisible();
  await expect(
    detail.getByRole("button", { name: "Start", exact: true }),
  ).toBeVisible();
  expect(
    await page.evaluate(
      () => document.documentElement.scrollWidth <= innerWidth,
    ),
  ).toBe(true);
});

test("one list keeps same-name services and schedules distinct across deep links and filters", async ({
  page,
}) => {
  await runtime.create("service", service("sample-shared"));
  await runtime.create("schedule", schedule("sample-shared"));
  await runtime.connect(page);
  await expect(
    page.getByRole("link", { name: "Inspect sample-shared" }),
  ).toHaveCount(2);
  await expect(
    page.getByRole("button", {
      name: /New service|New schedule|Add service|Add schedule/,
    }),
  ).toHaveCount(0);
  await page
    .getByRole("combobox", { name: "Filter kind" })
    .selectOption("schedule");
  await page
    .getByRole("combobox", { name: "Filter state" })
    .selectOption("paused");
  await page.getByRole("searchbox", { name: "Search work" }).fill("SHARED");
  await page.getByRole("link", { name: "Inspect sample-shared" }).click();
  const detail = page.getByRole("dialog", {
    name: "sample-shared",
    exact: true,
  });
  await expect(
    detail.getByRole("button", { name: "Run now", exact: true }),
  ).toBeVisible();
  await page.keyboard.press("Escape");
  await expect(page.getByRole("combobox", { name: "Filter kind" })).toHaveValue(
    "schedule",
  );
  await expect(
    page.getByRole("combobox", { name: "Filter state" }),
  ).toHaveValue("paused");
  await expect(
    page.getByRole("searchbox", { name: "Search work" }),
  ).toHaveValue("SHARED");
  await page.goto(`${runtime.origin}/#/services/sample-shared`);
  await expect(
    detail.getByRole("button", { name: "Start", exact: true }),
  ).toBeVisible();
  await page.reload();
  await expect(
    detail.getByRole("button", { name: "Start", exact: true }),
  ).toBeVisible();
  await page.keyboard.press("Escape");
  await expect(detail).toHaveCount(0);
  await expect(
    page.getByRole("link", { name: "Inspect sample-shared" }),
  ).toHaveCount(2);
  await page.getByRole("searchbox", { name: "Search work" }).fill("missing");
  await expect(page.getByText("No work matches these filters.")).toBeVisible();
  await expect(page.getByText("No work registered yet.")).toHaveCount(0);
});

test("uncertain completion blocks new work until explicit cleanup acknowledgement", async ({
  page,
}) => {
  const id = "sample-uncertain";
  const definition = service(id, "echo $$ > leader.pid; exec sleep 30");
  definition.working_directory = runtime.dir;
  await runtime.command({ operation: "create", definition });
  await runtime.command({ operation: "start", id });
  const history = await waitForValue(
    () => runtime.command({ operation: "history", id }),
    (v) => v.executions[0]?.supervisor_pid,
  );
  const execution = history.executions[0];
  const pid = await waitForValue(
    async () => {
      try {
        return Number(await readFile(join(runtime.dir, "leader.pid"), "utf8"));
      } catch {
        return 0;
      }
    },
    (v) => v > 0,
  );
  try {
    process.kill(execution.supervisor_pid, "SIGKILL");
    await waitForValue(
      () => runtime.command({ operation: "history", id }),
      (v) => v.executions[0]?.outcome?.type === "monitor_lost",
    );
    await runtime.connect(page);
    await page.getByRole("link", { name: `Inspect ${id}` }).click();
    const detail = page.getByRole("dialog", { name: id, exact: true });
    await expect(detail.getByText("Completion is uncertain")).toBeVisible();
    await expect(
      detail.getByRole("button", { name: "Start", exact: true }),
    ).toBeDisabled();
    await detail
      .getByRole("button", { name: "Acknowledge cleanup", exact: true })
      .click();
    const confirm = page.getByRole("dialog", { name: "Acknowledge cleanup?" });
    await expect(
      confirm.getByRole("button", { name: "Acknowledge cleanup", exact: true }),
    ).toBeDisabled();
    try {
      process.kill(-pid, "SIGKILL");
    } catch (error) {
      if ((error as NodeJS.ErrnoException).code !== "ESRCH") throw error;
    }
    await confirm
      .getByRole("checkbox", { name: /I inspected and cleaned up execution/ })
      .check();
    await confirm
      .getByRole("button", { name: "Acknowledge cleanup", exact: true })
      .click();
    await expect(detail.getByText("Completion is uncertain")).toHaveCount(0);
    await expect(
      detail.getByRole("button", { name: "Start", exact: true }),
    ).toBeEnabled();
    const after = await runtime.command({ operation: "history", id });
    expect(after.executions[0].outcome.type).toBe("monitor_lost");
    expect(after.executions[0].recovery_acknowledged_at_ms).not.toBeNull();
  } finally {
    try {
      process.kill(-pid, "SIGKILL");
    } catch {}
  }
});

test("lost connection retains the snapshot, disables mutations, and recovers on retry", async ({
  page,
}) => {
  await runtime.command({
    operation: "create",
    definition: service("sample-offline"),
  });
  await runtime.connect(page);
  await page.getByRole("link", { name: "Inspect sample-offline" }).click();
  const detail = page.getByRole("dialog", {
    name: "sample-offline",
    exact: true,
  });
  await expect(
    detail.getByRole("button", { name: "Start", exact: true }),
  ).toBeEnabled();
  await page.route("**/v1/browser/command", (r) => r.abort());
  await expect(
    detail.getByRole("button", { name: "Start", exact: true }),
  ).toBeDisabled();
  await expect(detail.getByRole("alert")).toContainText("could not refresh");
  await page.keyboard.press("Escape");
  await expect(detail).toHaveCount(0);
  await expect(page.getByRole("alert")).toContainText("last snapshot");
  await expect(
    page.getByRole("link", { name: "Inspect sample-offline" }),
  ).toBeVisible();
  expect(
    (await runtime.command({ operation: "history", id: "sample-offline" }))
      .executions,
  ).toHaveLength(0);
  await page.unroute("**/v1/browser/command");
  await page.getByRole("button", { name: "Retry", exact: true }).click();
  await expect(page.getByRole("alert")).toHaveCount(0);
  await page.getByRole("link", { name: "Inspect sample-offline" }).click();
  await expect(
    detail.getByRole("button", { name: "Start", exact: true }),
  ).toBeEnabled();
});

test("detail failures stay retryable and accepted commands are not reported as failures", async ({
  page,
}) => {
  const errors: string[] = [];
  page.on("pageerror", (error) => errors.push(error.message));
  await runtime.command({
    operation: "create",
    definition: service("sample-detail"),
  });
  await runtime.connect(page);
  let failDetail = true;
  await page.route("**/v1/browser/command", (route) => {
    const request = route.request().postDataJSON();
    if (failDetail && ["inspect", "history"].includes(request.operation))
      return route.fulfill({
        status: 503,
        json: { error: { code: "runtime_unavailable" } },
      });
    return route.continue();
  });
  await page.getByRole("link", { name: "Inspect sample-detail" }).click();
  const detail = page.getByRole("dialog", {
    name: "sample-detail",
    exact: true,
  });
  await expect(detail.getByRole("alert")).toContainText(
    "Couldn’t open this workload",
  );
  failDetail = false;
  await detail.getByRole("button", { name: "Retry", exact: true }).click();
  await expect(
    detail.getByRole("button", { name: "Start", exact: true }),
  ).toBeEnabled();
  failDetail = true;
  await detail.getByRole("button", { name: "Start", exact: true }).click();
  await expect(page.locator(".toast")).toContainText("Start requested");
  await expect(detail.getByRole("alert")).toContainText(
    "This detail could not refresh",
  );
  await expect(
    detail.getByRole("button", { name: "Restart", exact: true }),
  ).toBeDisabled();
  const accepted = await waitForValue(
    () => runtime.command({ operation: "history", id: "sample-detail" }),
    (value) => value.executions.length === 1,
  );
  expect(accepted.executions).toHaveLength(1);
  failDetail = false;
  await expect(
    detail.getByRole("button", { name: "Restart", exact: true }),
  ).toBeEnabled();
  await detail.getByRole("button", { name: "Restart", exact: true }).click();
  const confirm = page.getByRole("dialog", { name: "Restart this service?" });
  failDetail = true;
  await expect(
    confirm.getByRole("button", { name: "Restart", exact: true }),
  ).toBeDisabled();
  await expect(confirm.getByRole("alert")).toContainText(
    "Controls are disabled",
  );
  failDetail = false;
  await confirm.getByRole("button", { name: "Retry detail" }).click();
  await expect(
    confirm.getByRole("button", { name: "Restart", exact: true }),
  ).toBeEnabled();
  await confirm.getByRole("button", { name: "Keep as is" }).click();
  expect(errors).toEqual([]);
});

test("foreign browser origins cannot invoke local workload controls", async ({
  page,
}) => {
  const server = createServer((_req, res) => {
    res.writeHead(200, { "Content-Type": "text/html" });
    res.end("<!doctype html><title>Untrusted origin fixture</title>");
  });
  await new Promise<void>((resolve) => server.listen(0, "127.0.0.1", resolve));
  try {
    const address = server.address();
    if (!address || typeof address === "string")
      throw new Error("Fixture unavailable");
    await page.goto(`http://127.0.0.1:${address.port}`);
    const result = await page.evaluate(
      async ({ origin, definition }) => {
        try {
          await fetch(`${origin}/v1/browser/command`, {
            method: "POST",
            headers: {
              "Content-Type": "application/json",
              "X-Rhythm-Client": "1",
            },
            body: JSON.stringify({ operation: "create", definition }),
          });
          return "allowed";
        } catch {
          return "blocked";
        }
      },
      { origin: runtime.origin, definition: service("sample-cross-origin") },
    );
    expect(result).toBe("blocked");
    expect(
      (await runtime.command({ operation: "list" })).services,
    ).toHaveLength(0);
  } finally {
    await new Promise<void>((resolve, reject) =>
      server.close((error) => (error ? reject(error) : resolve())),
    );
  }
});
