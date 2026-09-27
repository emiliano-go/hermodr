export const fixture = { updated: false, failure: false, calls: 0, savedRetention: null as unknown };

export async function invoke<T>(command: string, args?: Record<string, unknown>): Promise<T> {
  if (command === "chat_settings") return {
    auto_download: null,
    retention: { max_age_hours: { kind: "inherit" }, max_messages: { kind: "limited", value: 200 }, on_demand: true },
  } as T;
  if (command === "set_chat_retention") {
    fixture.savedRetention = JSON.parse(JSON.stringify(args?.retention));
    return undefined as T;
  }
  if (command !== "boolean_props") throw new Error(`No synthetic response for ${command}`);
  fixture.calls++;
  if (fixture.failure) throw new Error("Synthetic disconnected account");
  return [
    { name: "example_enabled", code: 1, default: false, value: !fixture.updated },
    { name: "example_disabled", code: 2, default: true, value: false },
    { name: "example_missing", code: 3, default: true, value: null },
  ] as T;
}
