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

test("pairing, tab reload, single-use code, disconnect, and expired session", async ({
  page,
}) => {
  const code = await runtime.code();
  await page.goto(runtime.origin);
  await page.getByLabel("Paste your single-use browser code").fill("invalid");
  await page.getByRole("button", { name: "Connect to workspace" }).click();
  await expect(page.getByRole("alert")).toContainText(
    "invalid, expired, or already used",
  );
  await page.getByLabel("Paste your single-use browser code").fill(code);
  await page.getByRole("button", { name: "Connect to workspace" }).click();
  await expect(
    page.getByRole("heading", { name: "Background work, in view." }),
  ).toBeVisible();
  await page.reload();
  await expect(
    page.getByRole("heading", { name: "Background work, in view." }),
  ).toBeVisible();
  await page.getByRole("button", { name: "Disconnect browser" }).click();
  await expect(
    page.getByRole("heading", { name: "Good work keeps going." }),
  ).toBeVisible();
  await page.getByLabel("Paste your single-use browser code").fill(code);
  await page.getByRole("button", { name: "Connect to workspace" }).click();
  await expect(page.getByRole("alert")).toContainText(
    "invalid, expired, or already used",
  );
  await runtime.connect(page);
  await page.evaluate(async () => {
    const s = JSON.parse(sessionStorage.getItem("rhythm.browser-session")!);
    await fetch("/v1/browser/disconnect", {
      method: "POST",
      headers: { Authorization: `Bearer ${s.token}` },
    });
  });
  await page.getByRole("button", { name: "Refresh workspace" }).click();
  await expect(
    page.getByRole("heading", { name: "Good work keeps going." }),
  ).toBeVisible();
});

test("create service, start, inspect literal logs, cancel restart, restart, and stop", async ({
  page,
}) => {
  const errors: string[] = [];
  page.on("pageerror", (e) => errors.push(e.message));
  await runtime.connect(page);
  await page.getByRole("button", { name: "New service", exact: true }).click();
  const form = page.getByRole("dialog", { name: "New service" });
  await form.getByLabel("Name", { exact: false }).fill("sample-ui-service");
  await form.getByLabel("Executable", { exact: true }).fill("/bin/sh");
  await form
    .getByLabel("Arguments")
    .fill('-c\nprintf "<img src=x onerror=alert(1)>"; exec sleep 30');
  await form
    .getByRole("button", { name: "Create service", exact: true })
    .click();
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

test("create schedule, run now, history, resume, pause, and cancel queued work", async ({
  page,
}) => {
  await runtime.connect(page);
  await page.getByRole("button", { name: "New schedule", exact: true }).click();
  const form = page.getByRole("dialog", { name: "New schedule" });
  await form.getByLabel("Name", { exact: false }).fill("sample-ui-schedule");
  await form.getByLabel("Executable", { exact: true }).fill("/bin/sh");
  await form
    .getByLabel("Arguments")
    .fill("-c\nprintf schedule-output; exec sleep 30");
  await form.getByLabel("Every (seconds)").fill("0.5");
  await form.getByLabel("When already running").selectOption("queue_one");
  await form
    .getByRole("button", { name: "Create schedule", exact: true })
    .click();
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

test("daily and weekly creation, search filters, keyboard focus, responsive detail", async ({
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
  await runtime.connect(page);
  await page.getByRole("button", { name: "New schedule", exact: true }).click();
  const form = page.getByRole("dialog", { name: "New schedule" });
  await form.getByLabel("Name", { exact: false }).fill("sample-weekly");
  await form.getByLabel("Trigger", { exact: true }).selectOption("weekly");
  await form.getByLabel("Time", { exact: true }).fill("09:15");
  await form.getByLabel("IANA timezone").fill("UTC");
  await form.getByLabel("Fri", { exact: true }).check();
  await form
    .getByRole("button", { name: "Create schedule", exact: true })
    .click();
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
  await page.getByRole("button", { name: "New schedule", exact: true }).click();
  const daily = page.getByRole("dialog", { name: "New schedule" });
  await daily.getByLabel("Name", { exact: false }).fill("sample-daily");
  await daily.getByLabel("Trigger", { exact: true }).selectOption("daily");
  await daily.getByLabel("Time", { exact: true }).fill("18:30");
  await daily.getByLabel("IANA timezone").fill("UTC");
  await daily
    .getByRole("button", { name: "Create schedule", exact: true })
    .click();
  detail = page.getByRole("dialog", { name: "sample-daily", exact: true });
  await detail.getByRole("tab", { name: "Definition", exact: true }).click();
  await expect(detail.getByText("Daily at 18:30 · UTC")).toBeVisible();
  await page.keyboard.press("Escape");
  await expect(detail).toHaveCount(0);
  await page.getByRole("link", { name: "Overview", exact: true }).click();
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
  await page.getByRole("button", { name: "New service", exact: true }).click();
  const form = page.getByRole("dialog", { name: "New service" });
  await form.getByLabel("Name", { exact: false }).fill("sample-blocked");
  await page.route("**/v1/browser/command", (r) => r.abort());
  await expect(form.getByRole("alert")).toContainText("Connection lost");
  await expect(
    form.getByRole("button", { name: "Create service", exact: true }),
  ).toBeDisabled();
  await form.getByLabel("Name", { exact: false }).press("Enter");
  expect((await runtime.command({ operation: "list" })).services).toHaveLength(
    1,
  );
  await form.getByRole("button", { name: "Cancel", exact: true }).click();
  await expect(form).toHaveCount(0);
  await page.getByRole("button", { name: "Refresh workspace" }).click();
  await expect(page.getByRole("alert")).toContainText("last snapshot");
  await expect(
    page.getByRole("button", { name: "New service", exact: true }),
  ).toBeDisabled();
  await expect(
    page.getByRole("button", { name: "Add service", exact: true }),
  ).toBeDisabled();
  await expect(
    page.getByRole("button", { name: "Add schedule", exact: true }),
  ).toBeDisabled();
  await page.getByRole("link", { name: "Inspect sample-offline" }).click();
  await expect(page.getByRole("dialog").getByRole("alert")).toBeVisible();
  await page.keyboard.press("Escape");
  await expect(page.getByRole("dialog")).toHaveCount(0);
  await page.unroute("**/v1/browser/command");
  await page.getByRole("button", { name: "Retry", exact: true }).click();
  await expect(page.getByRole("alert")).toHaveCount(0);
  await expect(
    page.getByRole("link", { name: "Inspect sample-offline" }),
  ).toBeVisible();
  await expect(
    page.getByRole("button", { name: "New service", exact: true }),
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
