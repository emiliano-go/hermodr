import type { AttachmentRecovery } from "./models";

export function recoveryKey(record: AttachmentRecovery): string {
  const scope = record.context;
  return JSON.stringify([scope.accountId, scope.accountSeq, scope.generation, scope.chatGeneration, scope.chat, scope.batch]);
}

export function findRecovery(records: readonly AttachmentRecovery[], key: string): AttachmentRecovery | undefined {
  return records.find((record) => recoveryKey(record) === key);
}

export function saveAttachmentCopy(file: File) {
  const url = URL.createObjectURL(file);
  const anchor = document.createElement("a");
  anchor.href = url;
  anchor.download = file.name;
  document.body.append(anchor);
  anchor.click();
  anchor.remove();
  setTimeout(() => URL.revokeObjectURL(url), 1000);
}
