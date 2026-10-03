import type { CommandError, MessageParam, MessageRef } from "../utils/wire";
import { t } from "./localizer.ts";

const FALLBACK = "error.operation_failed";
const CODE = /^error\.[a-z0-9_]+(?:\.[a-z0-9_]+)*$/;
const record = (value: unknown): value is Record<string, unknown> =>
  value !== null && Object.prototype.toString.call(value) === "[object Object]";
const scalar = (value: unknown): value is MessageParam => value === null
  || typeof value === "string" || typeof value === "boolean"
  || (typeof value === "number" && Number.isFinite(value));
const params = (value: unknown): value is CommandError["params"] =>
  record(value) && Object.values(value).every(scalar);

function diagnostic(value: unknown): string {
  if (typeof value === "string") return value;
  const seen = new WeakSet<object>();
  try {
    return JSON.stringify(value, (_key, item) => {
      if (typeof item === "bigint") return String(item);
      if (typeof item === "number" && !Number.isFinite(item)) return String(item);
      if (item && typeof item === "object") {
        if (seen.has(item)) return "[Circular]";
        seen.add(item);
        if (item instanceof Error || (typeof item.message === "string" && typeof item.name === "string")) {
          return { name: item.name, message: item.message, stack: item.stack, cause: item.cause };
        }
      }
      return item;
    }) ?? String(value);
  } catch {
    try { return String(value); } catch { return ""; }
  }
}

export function messageText(message: MessageRef): string {
  const text = t(message.code, message.params);
  return text.trim() && text !== message.code && text !== t("locale.text_unavailable")
    ? text : t(FALLBACK, {});
}

export class LocalizedError extends Error {
  readonly descriptor: CommandError;

  constructor(descriptor: CommandError) {
    super();
    this.name = "LocalizedError";
    this.descriptor = descriptor;
    Object.defineProperty(this, "message", { configurable: true, get: () => messageText(this.descriptor) });
  }

  get code(): string { return this.descriptor.code; }
  get params(): CommandError["params"] { return this.descriptor.params; }
  get diagnostic(): string | undefined { return this.descriptor.diagnostic; }
  override toString(): string { return this.message; }
}

export function normalizeError(value: unknown): LocalizedError {
  if (value instanceof LocalizedError) return value;
  if (record(value) && value.kind === "postal_error" && typeof value.code === "string"
      && CODE.test(value.code) && params(value.params)
      && (value.diagnostic === undefined || typeof value.diagnostic === "string")) {
    return new LocalizedError({ kind: "postal_error", code: value.code, params: { ...value.params },
      ...(value.diagnostic === undefined ? {} : { diagnostic: value.diagnostic }) });
  }
  return new LocalizedError({ kind: "postal_error", code: FALLBACK, params: {}, diagnostic: diagnostic(value) });
}
