export const calls: string[] = [];
export async function invoke<T>(command: string, _args?: Record<string, unknown>): Promise<T> {
  calls.push(command);
  throw new Error(`IPC forbidden in album row fixture: ${command}`);
}
