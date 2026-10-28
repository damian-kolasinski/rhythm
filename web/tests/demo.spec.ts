import { test, expect } from "@playwright/test";
import { spawn } from "node:child_process";
import { createInterface } from "node:readline";
import { mkdir, rename } from "node:fs/promises";
import { binary } from "./runtime";

test("disposable demo uses real services, schedules, controls and logs", async ({
  browser,
}, testInfo) => {
  test.setTimeout(45000);
  const child = spawn(binary, ["--json", "demo", "--no-open"], {
    stdio: ["ignore", "pipe", "ignore"],
  });
  const lines = createInterface({ input: child.stdout! });
  const launch = await new Promise<{
    url: string;
    demo: boolean;
  }>((resolve, reject) => {
    const timer = setTimeout(
      () => reject(new Error("Demo readiness timed out")),
      8000,
    );
    lines.once("line", (line) => {
      clearTimeout(timer);
      try {
        resolve(JSON.parse(line));
      } catch {
        reject(new Error("Invalid demo launch"));
      }
    });
    child.once("exit", () => {
      clearTimeout(timer);
      reject(new Error("Demo exited before readiness"));
    });
  });
  const context = await browser.newContext({
    viewport: { width: 1440, height: 1050 },
    locale: "en-US",
    timezoneId: "UTC",
    reducedMotion: "reduce",
    recordVideo: {
      dir: testInfo.outputPath("video"),
      size: { width: 1440, height: 1050 },
    },
  });
  try {
    expect(launch.demo).toBe(true);
    const page = await context.newPage();
    const errors: string[] = [];
    page.on("pageerror", (e) => errors.push(e.message));
    await page.goto(launch.url);
    await expect(
      page.getByText("Demo workspace", { exact: true }),
    ).toBeVisible();
    await expect(
      page.getByRole("link", { name: "Inspect sample-indexer" }),
    ).toBeVisible();
    await page.getByRole("button", { name: "Refresh workspace" }).click();
    await expect(
      page
        .getByRole("row")
        .filter({ hasText: "sample-export" })
        .getByText("Failed", { exact: true }),
    ).toBeVisible();
    await page.waitForTimeout(900);
    await page.screenshot({
      path: testInfo.outputPath("overview.png"),
      fullPage: true,
    });
    await page.getByRole("link", { name: "Inspect sample-export" }).click();
    let detail = page.getByRole("dialog", {
      name: "sample-export",
      exact: true,
    });
    await detail
      .getByRole("button", { name: /View logs for execution/ })
      .first()
      .click();
    await expect(detail.getByTestId("stderr")).toContainText(
      "Export destination unavailable",
    );
    await page.waitForTimeout(900);
    await page.keyboard.press("Escape");
    await page.getByRole("link", { name: "Inspect sample-notifier" }).click();
    detail = page.getByRole("dialog", { name: "sample-notifier", exact: true });
    await detail.getByRole("button", { name: "Start", exact: true }).click();
    await expect(detail.locator(".status")).toHaveText("Running");
    await detail
      .getByRole("button", { name: /View logs for execution/ })
      .first()
      .click();
    await expect(detail.getByTestId("stdout")).toContainText("Notifier ready");
    await page.waitForTimeout(900);
    await detail.getByRole("button", { name: "Stop", exact: true }).click();
    await page
      .getByRole("dialog", { name: "Stop this service?" })
      .getByRole("button", { name: "Stop", exact: true })
      .click();
    await expect(detail.locator(".status")).toHaveText("Stopped");
    await page.keyboard.press("Escape");
    await page
      .getByRole("combobox", { name: "Filter kind" })
      .selectOption("schedule");
    await page.getByRole("link", { name: "Inspect sample-backup" }).click();
    detail = page.getByRole("dialog", { name: "sample-backup", exact: true });
    await detail.getByRole("button", { name: "Run now", exact: true }).click();
    await detail
      .getByRole("button", { name: /View logs for execution/ })
      .first()
      .click();
    await expect(detail.getByTestId("stdout")).toContainText(
      "Sample task completed",
    );
    await page.waitForTimeout(900);
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
    await page.keyboard.press("Escape");
    await page
      .getByRole("combobox", { name: "Filter kind" })
      .selectOption("all");
    await page.waitForTimeout(900);
    expect(errors).toEqual([]);
    const video = page.video()!;
    await context.close();
    if (process.env.RHYTHM_DEMO_OUTPUT) {
      await mkdir(process.env.RHYTHM_DEMO_OUTPUT, { recursive: true });
      await video.saveAs(`${process.env.RHYTHM_DEMO_OUTPUT}/walkthrough.webm`);
      await rename(
        testInfo.outputPath("overview.png"),
        `${process.env.RHYTHM_DEMO_OUTPUT}/overview.png`,
      );
    }
  } finally {
    await context.close();
    lines.close();
    child.kill("SIGTERM");
    await new Promise<void>((resolve, reject) => {
      if (child.exitCode !== null) return resolve();
      const timer = setTimeout(() => {
        child.kill("SIGKILL");
        reject(new Error("Demo did not shut down cleanly"));
      }, 7000);
      child.once("exit", (code) => {
        clearTimeout(timer);
        code === 0 ? resolve() : reject(new Error("Demo shutdown failed"));
      });
    });
  }
});
