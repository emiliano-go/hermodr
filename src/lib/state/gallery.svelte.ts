import type { GalleryCursor, GalleryFilter, GalleryItem, GalleryPage } from "../utils/wire";

export class GalleryState {
  items = $state<GalleryItem[]>([]);
  next = $state<GalleryCursor | null>(null);
  pageIndex = $state(0);
  loading = $state(false);
  error = $state<string | null>(null);
  private cursors: (GalleryCursor | null)[] = [null];
  private filter: GalleryFilter = { chat: null, kind: null, from_me: null, since: null, until: null };
  private sequence = 0;
  private attempted: { cursor: GalleryCursor | null; index: number } = { cursor: null, index: 0 };

  constructor(private fetchPage: (filter: GalleryFilter, cursor: GalleryCursor | null, limit: number) => Promise<GalleryPage>) {}

  resetAccount() {
    this.sequence++;
    this.items = [];
    this.next = null;
    this.pageIndex = 0;
    this.cursors = [null];
    this.loading = false;
    this.error = null;
    this.attempted = { cursor: null, index: 0 };
  }

  async reset(filter: GalleryFilter) {
    this.resetAccount();
    this.filter = filter;
    await this.load(null, 0);
  }

  async nextPage() {
    if (this.next && !this.loading) await this.load(this.next, this.pageIndex + 1);
  }

  async previousPage() {
    if (this.pageIndex > 0 && !this.loading) await this.load(this.cursors[this.pageIndex - 1], this.pageIndex - 1);
  }

  async retry() { if (!this.loading) await this.load(this.attempted.cursor, this.attempted.index); }

  private async load(cursor: GalleryCursor | null, index: number) {
    const sequence = ++this.sequence;
    this.attempted = { cursor, index };
    this.loading = true;
    this.error = null;
    try {
      const page = await this.fetchPage(this.filter, cursor, 60);
      if (sequence !== this.sequence) return;
      this.items = page.items;
      this.next = page.next_cursor;
      this.pageIndex = index;
      this.cursors = [...this.cursors.slice(0, index), cursor];
    } catch (error) {
      if (sequence === this.sequence) this.error = String(error);
    } finally {
      if (sequence === this.sequence) this.loading = false;
    }
  }
}
