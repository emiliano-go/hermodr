import type { RetentionLimit } from "./models.ts";

export function limitValue(limit: RetentionLimit): number | null {
  return limit.kind === "limited" ? limit.value : null;
}

export function limitKey(limit: RetentionLimit): string {
  return limit.kind === "limited" ? String(limit.value) : limit.kind;
}

export function parseLimit(key: string): RetentionLimit {
  if (key === "inherit") return { kind: "inherit" };
  if (key === "unlimited" || key === "") return { kind: "unlimited" };
  const value = Number(key);
  if (!Number.isSafeInteger(value) || value < 0 || value > 0xffffffff) throw new Error("Invalid retention limit");
  return { kind: "limited", value };
}
