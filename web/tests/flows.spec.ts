import { createServer } from "node:http";
import { readFile } from "node:fs/promises";
import { join } from "node:path";
import { test, expect } from "@playwright/test";
import { Runtime, schedule, waitForValue } from "./runtime";
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
    page.getByRole("heading", { name: "Schedules", exact: true }),
  ).toBeVisible();
  await expect(
    page.getByText("No schedules yet.", { exact: true }),
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
    page.getByText("No schedules yet.", { exact: true }),
  ).toBeVisible();
  const context = await browser.newContext();
  try {
    const second = await context.newPage();
    await second.goto(launch.url);
    await expect(
      second.getByText("No schedules yet.", { exact: true }),
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

test("CLI-created schedule runs with literal logs and explicit cancellation", async ({
  page,
}) => {
  const errors: string[] = [];
  page.on("pageerror", (e) => errors.push(e.message));
  await runtime.create(
    schedule(
      "sample-output",
      'printf "<img src=x onerror=alert(1)>"; exec sleep 30',
    ),
  );
  await runtime.connect(page);
  await expect(
    page.getByRole("button", {
      name: /New service|Add service|Create schedule/,
    }),
  ).toHaveCount(0);
  await page.getByRole("link", { name: "Inspect sample-output" }).click();
  const detail = page.getByRole("dialog", {
    name: "sample-output",
    exact: true,
  });
  await detail.getByRole("button", { name: "Run now", exact: true }).click();
  await expect(detail.locator(".status")).toHaveText("Running");
  await detail
    .getByRole("button", { name: /View logs for execution/ })
    .first()
    .click();
  await expect(detail.getByTestId("stdout")).toHaveText(
    "<img src=x onerror=alert(1)>",
  );
  await expect(detail.getByTestId("stdout").locator("img")).toHaveCount(0);
  await detail.getByRole("button", { name: "Cancel run", exact: true }).click();
  let confirm = page.getByRole("dialog", {
    name: "Cancel active and queued work?",
  });
  await confirm.getByRole("button", { name: "Keep as is" }).click();
  expect(
    (await runtime.command({ operation: "history", id: "sample-output" }))
      .executions,
  ).toHaveLength(1);
  await expect(detail.locator(".status")).toHaveText("Running");
  await detail.getByRole("button", { name: "Cancel run", exact: true }).click();
  confirm = page.getByRole("dialog", {
    name: "Cancel active and queued work?",
  });
  await confirm
    .getByRole("button", { name: "Cancel run", exact: true })
    .click();
  await expect(detail.locator(".status")).toHaveText("Paused");
  expect(errors).toEqual([]);
});

test("CLI-created schedule can run now, show history, resume, pause, and cancel queued work", async ({
  page,
}) => {
  await runtime.create(
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
    operation: "inspect",
    id: "sample-ui-schedule",
  });
  expect(stored.schedule.enabled).toBe(false);
  expect(stored.schedule.pending).toBeNull();
});

test("CLI calendar definitions, combined filters, keyboard focus, and responsive detail", async ({
  page,
}) => {
  await runtime.command({
    operation: "create",
    definition: schedule("sample-find"),
  });
  await runtime.command({
    operation: "create",
    definition: schedule("sample-hidden"),
  });
  await runtime.create({
    ...schedule("sample-weekly"),
    trigger: {
      type: "weekly",
      weekdays: [1, 5],
      hour: 9,
      minute: 15,
      timezone: "UTC",
    },
  });
  await runtime.create({
    ...schedule("sample-daily"),
    trigger: { type: "daily", hour: 18, minute: 30, timezone: "UTC" },
  });
  await runtime.connect(page);
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
  await expect(
    detail
      .getByRole("tabpanel", { name: "Definition", exact: true })
      .getByText("Mon, Fri at 09:15 · UTC"),
  ).toBeVisible();
  await page.keyboard.press("Escape");
  await expect(detail).toHaveCount(0);
  await page.getByRole("link", { name: "Inspect sample-daily" }).click();
  detail = page.getByRole("dialog", { name: "sample-daily", exact: true });
  await detail.getByRole("tab", { name: "Definition", exact: true }).click();
  await expect(
    detail
      .getByRole("tabpanel", { name: "Definition", exact: true })
      .getByText("Daily at 18:30 · UTC"),
  ).toBeVisible();
  await page.keyboard.press("Escape");
  await expect(detail).toHaveCount(0);
  await page.getByRole("searchbox", { name: "Search schedules" }).fill("find");
  await expect(
    page.getByRole("link", { name: "Inspect sample-find" }),
  ).toBeVisible();
  await expect(
    page.getByRole("link", { name: "Inspect sample-hidden" }),
  ).toHaveCount(0);
  await page.getByRole("searchbox", { name: "Search schedules" }).fill("");
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
    detail.getByRole("button", { name: "Run now", exact: true }),
  ).toBeVisible();
  expect(
    await page.evaluate(
      () => document.documentElement.scrollWidth <= innerWidth,
    ),
  ).toBe(true);
});

test("schedule detail links reload and preserve in-session search and state filters", async ({
  page,
}) => {
  await runtime.create(schedule("sample-linked"));
  await runtime.connect(page);
  await expect(page.getByRole("combobox", { name: "Filter kind" })).toHaveCount(
    0,
  );
  await page
    .getByRole("combobox", { name: "Filter state" })
    .selectOption("paused");
  await page
    .getByRole("searchbox", { name: "Search schedules" })
    .fill("LINKED");
  await page.getByRole("link", { name: "Inspect sample-linked" }).click();
  const detail = page.getByRole("dialog", {
    name: "sample-linked",
    exact: true,
  });
  await expect(
    detail.getByRole("button", { name: "Run now", exact: true }),
  ).toBeVisible();
  await expect(
    detail.getByRole("button", { name: /Start|Restart|Stop/, exact: true }),
  ).toHaveCount(0);
  await page.keyboard.press("Escape");
  await expect(detail).toHaveCount(0);
  await expect(
    page.getByRole("combobox", { name: "Filter state" }),
  ).toHaveValue("paused");
  await expect(
    page.getByRole("searchbox", { name: "Search schedules" }),
  ).toHaveValue("LINKED");
  await page.goto(`${runtime.origin}/#/sample-linked`);
  await expect(detail).toBeVisible();
  await page.reload();
  await expect(detail).toBeVisible();
  await page.keyboard.press("Escape");
  await expect(detail).toHaveCount(0);
  await page
    .getByRole("searchbox", { name: "Search schedules" })
    .fill("missing");
  await expect(
    page.getByText("No schedules match these filters."),
  ).toBeVisible();
  await expect(page.getByText("No schedules yet.")).toHaveCount(0);
});

test("uncertain completion blocks new work until explicit cleanup acknowledgement", async ({
  page,
}) => {
  const id = "sample-uncertain";
  const definition = schedule(id, "echo $$ > leader.pid; exec sleep 30");
  definition.working_directory = runtime.dir;
  await runtime.command({ operation: "create", definition });
  await runtime.command({ operation: "run_now", id });
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
      detail.getByRole("button", { name: "Run now", exact: true }),
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
      detail.getByRole("button", { name: "Run now", exact: true }),
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
    definition: schedule("sample-offline"),
  });
  await runtime.connect(page);
  await page.getByRole("link", { name: "Inspect sample-offline" }).click();
  const detail = page.getByRole("dialog", {
    name: "sample-offline",
    exact: true,
  });
  await expect(
    detail.getByRole("button", { name: "Run now", exact: true }),
  ).toBeEnabled();
  await page.route("**/v1/browser/command", (r) => r.abort());
  await expect(
    detail.getByRole("button", { name: "Run now", exact: true }),
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
    detail.getByRole("button", { name: "Run now", exact: true }),
  ).toBeEnabled();
});

test("detail failures stay retryable and accepted commands are not reported as failures", async ({
  page,
}) => {
  const errors: string[] = [];
  page.on("pageerror", (error) => errors.push(error.message));
  await runtime.command({
    operation: "create",
    definition: schedule("sample-detail", "exec sleep 30"),
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
    "Couldn’t open this schedule",
  );
  failDetail = false;
  await detail.getByRole("button", { name: "Retry", exact: true }).click();
  await expect(
    detail.getByRole("button", { name: "Run now", exact: true }),
  ).toBeEnabled();
  failDetail = true;
  await detail.getByRole("button", { name: "Run now", exact: true }).click();
  await expect(page.locator(".toast")).toContainText("Run now requested");
  await expect(detail.getByRole("alert")).toContainText(
    "This detail could not refresh",
  );
  await expect(
    detail.getByRole("button", { name: "Cancel run", exact: true }),
  ).toBeDisabled();
  const accepted = await waitForValue(
    () => runtime.command({ operation: "history", id: "sample-detail" }),
    (value) => value.executions.length === 1,
  );
  expect(accepted.executions).toHaveLength(1);
  failDetail = false;
  await expect(
    detail.getByRole("button", { name: "Cancel run", exact: true }),
  ).toBeEnabled();
  await detail.getByRole("button", { name: "Cancel run", exact: true }).click();
  const confirm = page.getByRole("dialog", {
    name: "Cancel active and queued work?",
  });
  failDetail = true;
  await expect(
    confirm.getByRole("button", { name: "Cancel run", exact: true }),
  ).toBeDisabled();
  await expect(confirm.getByRole("alert")).toContainText(
    "Controls are disabled",
  );
  failDetail = false;
  await confirm.getByRole("button", { name: "Retry detail" }).click();
  await expect(
    confirm.getByRole("button", { name: "Cancel run", exact: true }),
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
      { origin: runtime.origin, definition: schedule("sample-cross-origin") },
    );
    expect(result).toBe("blocked");
    expect(
      (await runtime.command({ operation: "list" })).schedules,
    ).toHaveLength(0);
  } finally {
    await new Promise<void>((resolve, reject) =>
      server.close((error) => (error ? reject(error) : resolve())),
    );
  }
});
