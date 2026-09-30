let handler: (command: string, args?: Record<string, unknown>) => unknown = () => { throw new Error("No synthetic handler"); };
export function setHandler(next: typeof handler) { handler = next; }
export async function invoke<T>(command: string, args?: Record<string, unknown>): Promise<T> {
  return await handler(command, args) as T;
}
export function log() {}
