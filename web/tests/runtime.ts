import { spawn, execFile, type ChildProcess } from "node:child_process";
import { promisify } from "node:util";
import { mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import type { Page } from "@playwright/test";
const execute = promisify(execFile);
export const binary =
  process.env.RHYTHM_BIN ??
  fileURLToPath(new URL("../../target/debug/rhythm", import.meta.url));
export class Runtime {
  constructor(
    public dir: string,
    public child: ChildProcess,
    public origin: string,
    private credential: string,
  ) {}
  static async start() {
    const dir = await mkdtemp(join(tmpdir(), "rhythm-browser-"));
    const child = spawn(
      binary,
      ["--data-dir", join(dir, "state"), "runtime", "run"],
      { stdio: "ignore" },
    );
    for (let i = 0; i < 250; i++) {
      try {
        const access = JSON.parse(
          await readFile(join(dir, "state", "access.json"), "utf8"),
        );
        const runtime = new Runtime(
          dir,
          child,
          `http://127.0.0.1:${access.port}`,
          access.token,
        );
        await runtime.command({ operation: "status" });
        return runtime;
      } catch {}
      await new Promise((r) => setTimeout(r, 20));
    }
    child.kill();
    await rm(dir, { recursive: true, force: true });
    throw new Error("Fixture runtime unavailable");
  }
  async command(body: unknown): Promise<any> {
    const response = await fetch(`${this.origin}/v1/command`, {
      method: "POST",
      headers: {
        "Content-Type": "application/json",
        Authorization: `Bearer ${this.credential}`,
      },
      body: JSON.stringify(body),
      signal: AbortSignal.timeout(5000),
    });
    if (!response.ok) throw new Error("Fixture request failed");
    return response.json();
  }
  async create(definition: { id: string; [field: string]: unknown }) {
    const file = join(this.dir, "schedule-definition.json");
    await writeFile(file, JSON.stringify(definition), { mode: 0o600 });
    await execute(binary, [
      "--json",
      "--data-dir",
      join(this.dir, "state"),
      "schedule",
      "create",
      "--file",
      file,
    ]);
  }
  async launch(): Promise<{ url: string; demo: boolean }> {
    const out = await execute(binary, [
      "--json",
      "--data-dir",
      join(this.dir, "state"),
      "ui",
      "--no-open",
    ]);
    return JSON.parse(out.stdout);
  }
  async connect(page: Page) {
    const launch = await this.launch();
    await page.goto(launch.url);
    await page
      .getByRole("heading", { name: "Schedules", exact: true })
      .waitFor();
  }
  async stop() {
    if (this.child.exitCode === null && this.child.signalCode === null) {
      try {
        await this.command({ operation: "shutdown" });
      } catch {}
      await new Promise<void>((resolve) => {
        if (this.child.exitCode !== null || this.child.signalCode !== null)
          return resolve();
        const timer = setTimeout(() => {
          this.child.kill("SIGKILL");
          resolve();
        }, 6000);
        this.child.once("exit", () => {
          clearTimeout(timer);
          resolve();
        });
      });
    }
    await rm(this.dir, { recursive: true, force: true });
  }
}
export function schedule(
  id: string,
  script = "printf schedule-output",
  every_ms = 1000,
) {
  return {
    id,
    command: ["/bin/sh", "-c", script],
    working_directory: "/tmp",
    trigger: { type: "interval", every_ms },
    overlap: "queue_one",
    missed: "run_once",
    grace_ms: 1000,
  };
}
export async function waitForValue<T>(
  get: () => Promise<T>,
  accept: (v: T) => boolean,
): Promise<T> {
  for (let i = 0; i < 200; i++) {
    const v = await get();
    if (accept(v)) return v;
    await new Promise((r) => setTimeout(r, 30));
  }
  throw new Error("Fixture state did not settle");
}
