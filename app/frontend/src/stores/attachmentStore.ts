import { createMemo, createSignal } from 'solid-js';
import { api } from '../services/api';
import type { AttachmentInfo } from '../types';
import { setAppError } from './notificationStore';
import { formatNumber, t } from '../i18n';

/** Files go over the IPC channel as a byte array; keep them small enough for that. */
export const MAX_ATTACHMENT_BYTES = 10 * 1024 * 1024;
/** More than this in one message is refused (the composer would become a file manager). */
export const MAX_ATTACHMENTS_PER_MESSAGE = 10;

export type PendingFile = {
  key: string;
  name: string;
  size: number;
  /** `toolarge` files are never uploaded. */
  status: 'uploading' | 'ready' | 'failed' | 'toolarge';
  info?: AttachmentInfo;
  error?: string;
  /** Kept while the upload can still be repeated. */
  file?: File;
};

/** Everything attached to the message being written, in the order it was added. */
export const [pendingFiles, setPendingFiles] = createSignal<PendingFile[]>([]);

/** The uploaded files that wait for the next message. */
export const pendingAttachments = createMemo(() =>
  pendingFiles().flatMap((entry) => (entry.status === 'ready' && entry.info ? [entry.info] : [])));
export const uploading = createMemo(() => pendingFiles().some((entry) => entry.status === 'uploading'));

/** Compatibility for callers that replace the whole list of uploaded files. */
export function setPendingAttachments(update: AttachmentInfo[] | ((previous: AttachmentInfo[]) => AttachmentInfo[])) {
  const next = typeof update === 'function' ? update(pendingAttachments()) : update;
  setPendingFiles(next.map((info) => ({ key: info.id, name: info.filename, size: info.size_bytes, status: 'ready' as const, info })));
}

const patch = (key: string, change: Partial<PendingFile>) =>
  setPendingFiles((previous) => previous.map((entry) => (entry.key === key ? { ...entry, ...change } : entry)));

async function upload(key: string, file: File) {
  patch(key, { status: 'uploading', error: undefined });
  try {
    const bytes = Array.from(new Uint8Array(await file.arrayBuffer()));
    const info = await api.uploadFile(file.name, file.type || 'application/octet-stream', bytes);
    patch(key, { status: 'ready', info, file: undefined });
  } catch (err) {
    patch(key, { status: 'failed', error: String(err).replace(/^Error:\s*/, '') });
  }
}

export async function attachFile(file: File) {
  const key = crypto.randomUUID();
  if (pendingFiles().length >= MAX_ATTACHMENTS_PER_MESSAGE) {
    setAppError(t('At most {count} files fit in one message', { count: MAX_ATTACHMENTS_PER_MESSAGE }));
    return;
  }
  if (file.size > MAX_ATTACHMENT_BYTES) {
    setPendingFiles((previous) => [...previous, { key, name: file.name, size: file.size, status: 'toolarge' }]);
    return;
  }
  setPendingFiles((previous) => [...previous, { key, name: file.name, size: file.size, status: 'uploading', file }]);
  await upload(key, file);
}

/** Several files at once (drag and drop, multi-select); they upload side by side. */
export async function attachFiles(files: Iterable<File>) {
  await Promise.all(Array.from(files).map((file) => attachFile(file)));
}

export async function retryUpload(key: string) {
  const entry = pendingFiles().find((candidate) => candidate.key === key);
  if (entry?.file && entry.status === 'failed') await upload(key, entry.file);
}

/** Remove a chip, by its key or by the id of the uploaded file. */
export function removePending(keyOrId: string) {
  setPendingFiles((previous) => previous.filter((entry) => entry.key !== keyOrId && entry.info?.id !== keyOrId));
}

/** The ids to send with the message; the composer's files are cleared (failed ones are dropped). */
export function takePending(): string[] {
  const ids = pendingAttachments().map((attachment) => attachment.id);
  setPendingFiles([]);
  return ids;
}

// --- showing attachments -----------------------------------------------------------------------

const THUMB_CACHE_LIMIT = 100;
const previews = new Map<string, Promise<string>>();

function remember(cache: Map<string, Promise<string>>, key: string, load: () => Promise<string>): Promise<string> {
  const known = cache.get(key);
  if (known) return known;
  const loading = load().catch((error) => { cache.delete(key); throw error; });
  cache.set(key, loading);
  if (cache.size > THUMB_CACHE_LIMIT) {
    const oldest = cache.keys().next().value;
    if (oldest !== undefined) {
      void cache.get(oldest)?.then((url) => URL.revokeObjectURL(url)).catch(() => {});
      cache.delete(oldest);
    }
  }
  return loading;
}

/** A `blob:` URL for the 320 px preview (fetched once, with our login). */
export function thumbUrl(attachment: AttachmentInfo): Promise<string> {
  return remember(previews, `thumb:${attachment.id}`, async () => {
    const bytes = await api.downloadThumb(attachment.id);
    return URL.createObjectURL(new Blob([bytes], { type: 'image/png' }));
  });
}

/** A `blob:` URL of the original image, for the viewer. */
export function imageUrl(attachment: AttachmentInfo): Promise<string> {
  return remember(previews, `full:${attachment.id}`, async () => {
    const bytes = await api.downloadFile(attachment.id);
    return URL.createObjectURL(new Blob([bytes], { type: attachment.content_type }));
  });
}

export const isImage = (attachment: AttachmentInfo) => attachment.content_type.startsWith('image/');

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
    setAppError(t('Download failed: {error}', { error: String(err) }));
  }
}

export function formatSize(bytes: number): string {
  if (bytes < 1024) return `${formatNumber(bytes)} B`;
  if (bytes < 1024 * 1024) return `${formatNumber(Math.max(1, Math.round(bytes / 1024)))} KB`;
  return `${formatNumber(bytes / 1024 / 1024, 1)} MB`;
}
