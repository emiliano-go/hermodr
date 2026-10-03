import assert from "node:assert/strict";
import test from "node:test";
import { sqlcipherEnvironment } from "../../scripts/sqlcipher-env.mjs";

test("Windows Tauri builds reuse complete installed static OpenSSL without changing parent environment", () => {
  const source = { ProgramFiles: "C:/Programs", PATH: "unchanged", OTHER: "preserved" };
  const env = sqlcipherEnvironment(source, "win32", "x64", () => true);
  assert.equal(env.OPENSSL_NO_VENDOR, "1");
  assert.equal(env.OPENSSL_STATIC, "1");
  assert.equal(env.OPENSSL_LIBS, "libssl_static:libcrypto_static");
  assert.match(env.OPENSSL_LIB_DIR!, /VC[\\/]x64[\\/]MD$/);
  assert.equal(env.PATH, source.PATH);
  assert.deepEqual(source, { ProgramFiles: "C:/Programs", PATH: "unchanged", OTHER: "preserved" });
  const cross = sqlcipherEnvironment({ CARGO_BUILD_TARGET: "x86_64-pc-windows-msvc" }, "win32", "arm64", () => true);
  assert.match(cross.OPENSSL_LIB_DIR!, /VC[\\/]x64[\\/]MD$/);
  const dynamic = sqlcipherEnvironment({ OPENSSL_STATIC: "0", PATH: "preserved" }, "win32", "x64", (path: string) => !path.endsWith("_static.lib"));
  assert.equal(dynamic.OPENSSL_LIBS, "libssl:libcrypto");
  assert.ok(dynamic.PATH?.endsWith(";preserved"));
  assert.deepEqual(sqlcipherEnvironment({ OPENSSL_STATIC: "0" }, "win32", "x64", (path: string) => path.endsWith("_static.lib") || path.endsWith("opensslv.h")), { OPENSSL_STATIC: "0" });
});

test("explicit providers, unsupported targets, missing SDKs and other hosts keep existing build choices", () => {
  for (const [env, platform, exists] of [
    [{ OPENSSL_DIR: "custom" }, "win32", () => true],
    [{ OPENSSL_NO_VENDOR: "0" }, "win32", () => true],
    [{ CARGO_BUILD_TARGET: "x86_64-unknown-linux-gnu" }, "win32", () => true],
    [{}, "win32", () => false], [{}, "linux", () => true], [{}, "darwin", () => true],
  ] as const) assert.deepEqual(sqlcipherEnvironment(env, platform, "x64", exists), env);
});
