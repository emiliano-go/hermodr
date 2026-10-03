import { existsSync } from "node:fs";
import { join } from "node:path";

/** @param {Record<string, string | undefined>} env @param {string} platform @param {string} arch @param {(path: string) => boolean} exists */
export function sqlcipherEnvironment(env, platform, arch, exists = existsSync) {
  const result = { ...env };
  if (platform !== "win32" || env.OPENSSL_DIR || env.OPENSSL_LIB_DIR || env.OPENSSL_INCLUDE_DIR
      || env.OPENSSL_NO_VENDOR === "0") return result;
  const target = env.CARGO_BUILD_TARGET;
  if (target && !target.includes("windows-msvc")) return result;
  if (target && !target.startsWith("x86_64-") && !target.startsWith("aarch64-")) return result;
  if (!target && !["x64", "arm64"].includes(arch)) return result;
  const machine = target ? (target.startsWith("aarch64-") ? "arm64" : "x64") : (arch === "arm64" ? "arm64" : "x64");
  const roots = [join(env.ProgramFiles || "C:/Program Files", "OpenSSL-Win64"), "C:/OpenSSL-Win64"];
  for (const root of roots) {
    const include = join(root, "include"), lib = join(root, "lib", "VC", machine, "MD");
    const libraries = env.OPENSSL_STATIC === "0" ? ["libcrypto.lib", "libssl.lib"] : ["libcrypto_static.lib", "libssl_static.lib"];
    if (![join(include, "openssl", "opensslv.h"), ...libraries.map((name) => join(lib, name))].every(exists)) continue;
    result.OPENSSL_NO_VENDOR ??= "1";
    result.OPENSSL_DIR = root;
    result.OPENSSL_INCLUDE_DIR = include;
    result.OPENSSL_LIB_DIR = lib;
    result.OPENSSL_STATIC ??= "1";
    result.OPENSSL_LIBS ??= result.OPENSSL_STATIC === "0" ? "libssl:libcrypto" : "libssl_static:libcrypto_static";
    if (result.OPENSSL_STATIC === "0") result.PATH = `${join(root, "bin")};${result.PATH || ""}`;
    return result;
  }
  return result;
}
