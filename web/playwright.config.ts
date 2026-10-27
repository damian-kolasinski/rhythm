import { defineConfig } from "@playwright/test";
export default defineConfig({
  testDir: "./tests",
  timeout: 30000,
  expect: {
    timeout: 8000,
    toHaveScreenshot: { maxDiffPixelRatio: 0.003, animations: "disabled" },
  },
  fullyParallel: false,
  workers: 1,
  retries: 0,
  use: {
    browserName: "chromium",
    viewport: { width: 1440, height: 1050 },
    locale: "en-US",
    timezoneId: "UTC",
    colorScheme: "light",
    reducedMotion: "reduce",
    trace: "off",
    video: "off",
    screenshot: "only-on-failure",
  },
  reporter: [["list"]],
  snapshotPathTemplate: "{testDir}/goldens/{arg}{ext}",
});
