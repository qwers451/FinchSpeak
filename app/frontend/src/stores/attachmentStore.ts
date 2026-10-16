import { createSignal } from 'solid-js';
import { api } from '../services/api';
import type { AttachmentInfo } from '../types';
import { setAppError } from './notificationStore';

/** Files go over the IPC channel as a byte array; keep them small enough for that. */
export const MAX_ATTACHMENT_BYTES = 10 * 1024 * 1024;

/** Files uploaded and waiting to be sent with the next message. */
export const [pendingAttachments, setPendingAttachments] = createSignal<AttachmentInfo[]>([]);
export const [uploading, setUploading] = createSignal(false);

export async function attachFile(file: File) {
  if (file.size > MAX_ATTACHMENT_BYTES) {
    setAppError(`"${file.name}" is larger than ${MAX_ATTACHMENT_BYTES / 1024 / 1024} MB`);
    return;
  }
  setUploading(true);
  try {
    const bytes = Array.from(new Uint8Array(await file.arrayBuffer()));
    const info = await api.uploadFile(file.name, file.type || 'application/octet-stream', bytes);
    setPendingAttachments((previous) => [...previous, info]);
  } catch (err) {
    setAppError(`Upload failed: ${String(err)}`);
  } finally {
    setUploading(false);
  }
}

export function removePending(id: string) {
  setPendingAttachments((previous) => previous.filter((attachment) => attachment.id !== id));
}

export function takePending(): string[] {
  const ids = pendingAttachments().map((attachment) => attachment.id);
  setPendingAttachments([]);
  return ids;
}

/** Fetch an attachment (with our login) and hand it to the browser as a download. */
export async function downloadAttachment(attachment: AttachmentInfo) {
  try {
    const bytes = await api.downloadFile(attachment.id);
    const url = URL.createObjectURL(new Blob([bytes], { type: attachment.content_type }));
    const link = document.createElement('a');
    link.href = url;
    link.download = attachment.filename;
    link.click();
    setTimeout(() => URL.revokeObjectURL(url), 10_000);
  } catch (err) {
    setAppError(`Download failed: ${String(err)}`);
  }
}
