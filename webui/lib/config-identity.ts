function randomToken(): string {
  if (typeof crypto !== "undefined" && "getRandomValues" in crypto) {
    const bytes = new Uint8Array(8);
    crypto.getRandomValues(bytes);
    return Array.from(bytes, (byte) => byte.toString(16).padStart(2, "0")).join(
      "",
    );
  }
  return Math.random().toString(36).slice(2, 12);
}

export function createSnapshotId(): string {
  return `snap_${Date.now().toString(36)}_${randomToken()}`;
}
