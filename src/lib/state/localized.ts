import { LocalizedError, messageText, normalizeError } from "../i18n/errors.ts";
import { hasMessage, type MessageParams } from "../i18n/localizer.ts";
import type { MessageFailure, MessageRef } from "../utils/wire.ts";

export const uiMessage = (code: string, params: MessageParams = {}): MessageRef => ({ code, params });
export const displayMessage = (message: MessageRef | string): string =>
  typeof message === "string" ? message : messageText(message);

export function isUiMessage(value: unknown): value is MessageRef {
  if (!value || typeof value !== "object") return false;
  const message = value as Record<string, unknown>;
  return typeof message.code === "string" && /^[a-z][a-z0-9_]*(?:\.[a-z0-9_]+)+$/.test(message.code)
    && !!message.params && typeof message.params === "object" && !Array.isArray(message.params)
    && Object.values(message.params).every((param) => param === null || ["string", "number", "boolean"].includes(typeof param));
}

export function localizedFailure(value: unknown, fallbackCode: string, legacyDiagnostic?: string): LocalizedError {
  if (isUiMessage(value) && hasMessage(value.code)) {
    const diagnostic = (value as MessageFailure).diagnostic;
    if (diagnostic == null || typeof diagnostic === "string") {
      if (!diagnostic && legacyDiagnostic) return uiError(value.code, value.params, legacyDiagnostic);
      return new LocalizedError({ kind: "postal_error", code: value.code, params: value.params,
        ...(diagnostic ? { diagnostic } : {}) });
    }
  }
  return uiError(fallbackCode, {}, legacyDiagnostic === undefined || legacyDiagnostic === value ? value : { reference: value, legacy: legacyDiagnostic });
}

export function uiError(code: string, params: MessageParams = {}, cause?: unknown): LocalizedError {
  const failure = cause === undefined ? undefined : normalizeError(cause);
  return new LocalizedError({ kind: "postal_error", code, params,
    ...(failure ? { diagnostic: failure.diagnostic ?? JSON.stringify(failure.descriptor) } : {}) });
}
