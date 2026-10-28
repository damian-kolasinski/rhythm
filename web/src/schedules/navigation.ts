// Only the list and a single schedule detail are addressable.
export function scheduleRoute(hash: string): string | undefined {
  const encoded = hash.replace(/^#\/?/, "");
  try {
    const id = decodeURIComponent(encoded);
    return /^[a-z][a-z0-9_-]{0,62}$/.test(id) ? id : undefined;
  } catch {
    return undefined;
  }
}
export function detailHash(id: string): string {
  return `/${encodeURIComponent(id)}`;
}
