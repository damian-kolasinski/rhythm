export class ApiError extends Error {
  constructor(public code: string) {
    super(code);
  }
}
export function errorMessage(error: unknown): string {
  const code = error instanceof ApiError ? error.code : "unavailable";
  return (
    (
      {
        unauthorized:
          "The runtime refused this request. Open its local workspace URL and retry.",
        not_found:
          "This workload is no longer available. Refresh the workspace.",
        already_exists: "That name is already in use. Choose another name.",
        busy: "A run is already active or queued. Wait for it to finish, or cancel it first.",
        recovery_required:
          "This execution needs investigation before new work can start.",
        invalid_request:
          "Check the definition. Use an absolute executable and working directory, and valid schedule settings.",
        invalid_response:
          "The runtime returned an unsupported response. Refresh or update the runtime.",
      } as Record<string, string>
    )[code] ??
    "The runtime is unavailable. Check that it is running, then retry."
  );
}
