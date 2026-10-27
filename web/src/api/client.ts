import { z } from "zod";
import { workspaceInfoSchema } from "./contracts";
import { ApiError } from "./errors";
async function send(path: string, body: unknown): Promise<unknown> {
  let response: Response;
  try {
    response = await fetch(path, {
      method: "POST",
      headers: {
        "Content-Type": "application/json",
        "X-Rhythm-Client": "1",
      },
      body: JSON.stringify(body),
      cache: "no-store",
      signal: AbortSignal.timeout(8000),
    });
  } catch {
    throw new ApiError("unavailable");
  }
  let data: unknown;
  try {
    data = await response.json();
  } catch {
    throw new ApiError("invalid_response");
  }
  if (!response.ok) {
    const parsed = z
      .object({ error: z.object({ code: z.string() }) })
      .safeParse(data);
    throw new ApiError(
      response.status === 401
        ? "unauthorized"
        : parsed.success
          ? parsed.data.error.code
          : "unavailable",
    );
  }
  return data;
}
export function createApi() {
  return async <T>(body: unknown, schema: z.ZodType<T>): Promise<T> => {
    const parsed = schema.safeParse(await send("/v1/browser/command", body));
    if (!parsed.success) throw new ApiError("invalid_response");
    return parsed.data;
  };
}
export type Api = ReturnType<typeof createApi>;
export async function workspaceInfo() {
  const parsed = workspaceInfoSchema.safeParse(
    await send("/v1/browser/info", {}),
  );
  if (!parsed.success) throw new ApiError("invalid_response");
  return parsed.data;
}
