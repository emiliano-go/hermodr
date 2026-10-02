import { invoke } from "$lib/utils/ipc";
import type { SpaceAction, SpaceResolution, SpaceSelection, SpaceSnapshot, SpaceTarget } from "$lib/utils/wire";
import { messages } from "$lib/state/messages.svelte";
import { session } from "$lib/state/session.svelte";
import { targetKey } from "./spaces";

export class SpacesState {
  account = $state<string | null>(null);
  snapshot = $state.raw<SpaceSnapshot>({ spaces: [], items: [] });
  selected = $state.raw<SpaceSelection>({ kind: "all" });
  resolution = $state.raw<SpaceResolution | null>(null);
  loaded = $state(false);
  loading = $state(false);
  busy = $state(false);
  error = $state<string | null>(null);
  keywordCounts: () => Record<string, number> = () => ({});
  private generation = -1;
  private epoch = 0;
  private request = 0;

  reset() {
    this.epoch++;
    this.request++;
    this.account = null;
    this.generation = -1;
    this.snapshot = { spaces: [], items: [] };
    this.selected = { kind: "all" };
    this.resolution = null;
    this.loaded = this.loading = this.busy = false;
    this.error = null;
  }

  private scope() {
    const account = session.activeAccount, generation = messages.accountGeneration, epoch = this.epoch;
    return { account, current: () => !!account && account === this.account && account === session.activeAccount
      && generation === this.generation && generation === messages.accountGeneration && epoch === this.epoch };
  }

  private keepSelection() {
    const selected = this.selected;
    if (selected.kind === "space" && !this.snapshot.spaces.some((space) => space.id === selected.space_id))
      this.selected = { kind: "all" };
  }

  async refresh() {
    const account = session.activeAccount, generation = messages.accountGeneration;
    if (!account) { this.reset(); return; }
    if (this.account !== account || this.generation !== generation) this.reset();
    this.account = account;
    this.generation = generation;
    const scope = this.scope(), request = ++this.request;
    const keywordCounts = { ...this.keywordCounts() };
    const current = () => scope.current() && request === this.request;
    this.loading = true;
    this.resolution = null;
    this.error = null;
    try {
      const snapshot = await invoke<SpaceSnapshot>("spaces_snapshot", { accountId: account });
      if (!current()) return;
      this.snapshot = snapshot;
      this.loaded = true;
      this.keepSelection();
      const selection = structuredClone(this.selected);
      const resolution = await invoke<SpaceResolution>("resolve_spaces", { accountId: account, selection, keywordCounts });
      if (current()) this.resolution = resolution;
    } catch (failure) { if (current()) this.error = String(failure); }
    finally { if (current()) this.loading = false; }
  }

  async select(selection: SpaceSelection) {
    this.selected = structuredClone(selection);
    await this.resolve();
  }

  async resolve() {
    const scope = this.scope();
    if (!scope.current()) return;
    const request = ++this.request, selection = structuredClone(this.selected);
    const keywordCounts = { ...this.keywordCounts() };
    const current = () => scope.current() && request === this.request;
    this.loading = true;
    this.resolution = null;
    this.error = null;
    try {
      const resolution = await invoke<SpaceResolution>("resolve_spaces", { accountId: scope.account, selection, keywordCounts });
      if (current()) this.resolution = resolution;
    } catch (failure) { if (current()) this.error = String(failure); }
    finally { if (current()) this.loading = false; }
  }

  private async write(command: "spaces_action" | "import_space_metadata", args: Record<string, unknown>) {
    const scope = this.scope();
    if (!scope.current() || this.busy) throw new Error("Spaces are unavailable for this account.");
    this.request++;
    this.loading = false;
    this.busy = true;
    this.error = null;
    try {
      const snapshot = await invoke<SpaceSnapshot>(command, { ...args, accountId: scope.account });
      if (!scope.current()) throw new Error("Account changed during Space operation.");
      this.request++;
      this.snapshot = snapshot;
      this.loaded = true;
      this.keepSelection();
      await this.resolve();
    } catch (failure) {
      if (scope.current()) this.error = String(failure);
      throw failure;
    } finally { if (scope.current()) this.busy = false; }
  }

  mutate(action: SpaceAction) { return this.write("spaces_action", { action: structuredClone(action) }); }

  async add(spaceId: string, targets: SpaceTarget[]) {
    const scope = this.scope(), captured = structuredClone(targets);
    for (const target of captured) {
      if (!scope.current()) throw new Error("Account changed during Space operation.");
      if (this.snapshot.items.some((item) => item.space_id === spaceId && targetKey(item.target) === targetKey(target))) continue;
      await this.mutate({ kind: "add_item", id: crypto.randomUUID(), space_id: spaceId, target });
    }
  }

  async exportMetadata(): Promise<string> {
    const scope = this.scope();
    if (!scope.current()) throw new Error("Spaces are unavailable for this account.");
    const json = await invoke<string>("export_space_metadata", { accountId: scope.account });
    if (!scope.current()) throw new Error("Account changed during Space export.");
    return json;
  }

  importMetadata(json: string) { return this.write("import_space_metadata", { json }); }
}

export const spaces = new SpacesState();
