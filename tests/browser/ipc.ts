export const fixture = { updated: false, failure: false, calls: 0 };

export async function invoke<T>(command: string): Promise<T> {
  if (command !== "boolean_props") throw new Error(`No synthetic response for ${command}`);
  fixture.calls++;
  if (fixture.failure) throw new Error("Synthetic disconnected account");
  return [
    { name: "example_enabled", code: 1, default: false, value: !fixture.updated },
    { name: "example_disabled", code: 2, default: true, value: false },
    { name: "example_missing", code: 3, default: true, value: null },
  ] as T;
}
