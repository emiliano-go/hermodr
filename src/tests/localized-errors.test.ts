import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";
import { runInNewContext } from "node:vm";
import ts from "typescript";

function adapter() {
  let locale = "en";
  const catalogs: Record<string, Record<string, string>> = {
    en: { "locale.text_unavailable": "Text unavailable.", "error.operation_failed": "Operation failed.", "error.too_large": "Maximum {max}; received {actual}." },
    es: { "locale.text_unavailable": "Texto no disponible.", "error.operation_failed": "La operación falló." },
  };
  const source = readFileSync(new URL("../lib/i18n/errors.ts", import.meta.url), "utf8");
  const output = ts.transpileModule(source, { compilerOptions: { target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.CommonJS } }).outputText;
  const exports: Record<string, any> = {};
  runInNewContext(output, { exports, Error, require: (name: string) => {
    assert.equal(name, "./localizer.ts");
    return { t: (code: string, params: Record<string, unknown>) => {
      const template = catalogs[locale]?.[code] ?? catalogs.en[code]
        ?? catalogs[locale]?.["locale.text_unavailable"] ?? catalogs.en["locale.text_unavailable"];
      return template.replace(/\{(\w+)\}/g, (_match, key) => String(params[key]));
    } };
  } });
  return { normalize: exports.normalizeError, locale: (value: string) => { locale = value; } };
}

test("typed errors preserve scalar facts and diagnostic without displaying debug text", () => {
  const { normalize } = adapter();
  const input = { kind: "postal_error", code: "error.too_large", params: { max: 10, actual: 12, item: "file", retry: false, missing: null }, diagnostic: "technical cause" };
  const error = normalize(input);
  assert.equal(error.code, input.code);
  assert.equal(JSON.stringify(error.params), JSON.stringify(input.params));
  assert.equal(error.diagnostic, "technical cause");
  assert.equal(error.message, "Maximum 10; received 12.");
  assert.equal(String(error), error.message);
  assert.equal(normalize(error), error);
  input.params.actual = 99;
  assert.equal(error.params.actual, 12);
});

test("existing errors retranslate after locale changes and use English for missing locale entries", () => {
  const { normalize, locale } = adapter();
  const generic = normalize("socket closed");
  const specific = normalize({ kind: "postal_error", code: "error.too_large", params: { max: 1, actual: 2 } });
  locale("es");
  assert.equal(generic.message, "La operación falló.");
  assert.equal(String(generic), "La operación falló.");
  assert.equal(specific.message, "Maximum 1; received 2.");
  assert.equal(generic.diagnostic, "socket closed");
});

test("unknown codes stay in descriptors while user text uses localized generic fallback", () => {
  const { normalize, locale } = adapter();
  const error = normalize({ kind: "postal_error", code: "error.future_failure", params: { status: 409 }, diagnostic: "server conflict" });
  locale("es");
  assert.equal(error.code, "error.future_failure");
  assert.equal(error.params.status, 409);
  assert.equal(error.diagnostic, "server conflict");
  assert.equal(error.message, "La operación falló.");
});

test("legacy and malformed failures never expose raw keys or object coercion", () => {
  const { normalize } = adapter();
  const cause = new Error("connection refused");
  const error = normalize(new Error("download failed", { cause }));
  assert.match(error.diagnostic, /download failed/);
  assert.match(error.diagnostic, /connection refused/);
  for (const value of ["not connected yet", undefined, null, 42,
    { message: "ACL denied", command: "float_context" },
    { kind: "postal_error", code: "invalid key", params: {} },
    { kind: "postal_error", code: "error.too_large", params: { max: { nested: true } } },
    { kind: "postal_error", code: "error.too_large", params: { max: Infinity } },
    { kind: "postal_error", code: "error.too_large", params: [] },
    { kind: "postal_error", code: "error.too_large", params: {}, diagnostic: {} }]) {
    const normalized = normalize(value);
    assert.equal(normalized.code, "error.operation_failed");
    assert.equal(normalized.message, "Operation failed.");
    assert.equal(String(normalized), "Operation failed.");
    assert.equal(typeof normalized.diagnostic, "string");
  }
  assert.match(normalize({ message: "ACL denied", command: "float_context" }).diagnostic, /float_context/);
});

test("cyclic diagnostics and bigint details survive normalization", () => {
  const { normalize } = adapter();
  const failure: Record<string, unknown> = { offset: 12n, status: 503 };
  failure.self = failure;
  const error = normalize(failure);
  assert.match(error.diagnostic, /12/);
  assert.match(error.diagnostic, /503/);
  assert.match(error.diagnostic, /Circular/);
  assert.equal(error.message, "Operation failed.");
});
