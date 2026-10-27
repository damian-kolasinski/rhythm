import { z } from "zod";
import { sessionSchema } from "./contracts";
import { ApiError } from "./errors";
async function send(
  path: string,
  body: unknown,
  token?: string,
): Promise<unknown> {
  let response: Response;
  try {
    response = await fetch(path, {
      method: "POST",
      headers: {
        "Content-Type": "application/json",
        ...(token ? { Authorization: `Bearer ${token}` } : {}),
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
export function createApi(token: string, expired: () => void) {
  return async <T>(body: unknown, schema: z.ZodType<T>): Promise<T> => {
    try {
      const data = await send("/v1/browser/command", body, token);
      const parsed = schema.safeParse(data);
      if (!parsed.success) throw new ApiError("invalid_response");
      return parsed.data;
    } catch (error) {
      if (error instanceof ApiError && error.code === "unauthorized") expired();
      throw error;
    }
  };
}
export type Api = ReturnType<typeof createApi>;
export async function pair(code: string) {
  const parsed = sessionSchema.safeParse(
    await send("/v1/browser/session", { code }),
  );
  if (!parsed.success) throw new ApiError("invalid_response");
  return parsed.data;
}
export async function disconnect(token: string) {
  await send("/v1/browser/disconnect", {}, token);
}
