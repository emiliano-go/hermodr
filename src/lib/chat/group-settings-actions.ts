import { normalizeError, type LocalizedError } from "../i18n/errors.ts";
import type { GroupSettings } from "$lib/utils/wire";

export async function saveGroupMetadata(
  write: () => Promise<void>,
  reload: () => Promise<GroupSettings>,
  current: () => boolean,
  acknowledged: () => void,
): Promise<{ snapshot: GroupSettings | null; refreshError: LocalizedError | null }> {
  if (!current()) return { snapshot: null, refreshError: null };
  await write();
  if (!current()) return { snapshot: null, refreshError: null };
  acknowledged();
  try {
    const snapshot = await reload();
    return { snapshot: current() ? snapshot : null, refreshError: null };
  } catch (error) {
    return { snapshot: null, refreshError: current() ? normalizeError(error) : null };
  }
}
