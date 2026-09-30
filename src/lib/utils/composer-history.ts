export type DraftSnapshot = {
  text: string;
  start: number;
  end: number;
  mentions: { name: string; jid: string }[];
};

export class ComposerHistory {
  private past: DraftSnapshot[] = [];
  private future: DraftSnapshot[] = [];
  private group: { kind: string; time: number; after: DraftSnapshot } | null = null;

  record(before: DraftSnapshot, after: DraftSnapshot, kind = "", time = Date.now()) {
    if (before.text === after.text) return;
    const groupable = ["insertText", "deleteContentBackward", "deleteContentForward"].includes(kind);
    const previous = this.group;
    if (!groupable || !previous || previous.kind !== kind || time - previous.time > 1000 ||
      before.text !== previous.after.text || before.start !== before.end ||
      before.start !== previous.after.start || before.end !== previous.after.end) {
      this.past.push(before);
      if (this.past.length > 100) this.past.shift();
    }
    this.future = [];
    this.group = groupable ? { kind, time, after } : null;
  }

  undo(current: DraftSnapshot): DraftSnapshot | undefined {
    this.group = null;
    const previous = this.past.pop();
    if (previous) this.future.push(current);
    return previous;
  }

  redo(current: DraftSnapshot): DraftSnapshot | undefined {
    this.group = null;
    const next = this.future.pop();
    if (next) this.past.push(current);
    return next;
  }

  reset() {
    this.past = [];
    this.future = [];
    this.group = null;
  }
}
