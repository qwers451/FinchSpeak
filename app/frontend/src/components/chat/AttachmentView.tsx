import { createResource, createSignal, For, Show } from 'solid-js';
import { downloadAttachment, formatSize, imageUrl, isImage, thumbUrl } from '../../stores/attachmentStore';
import type { AttachmentInfo } from '../../types';
import { Button, Icon, Modal } from '../../ui';

/** Longest side of a preview on screen. */
const PREVIEW_MAX_WIDTH = 320;
const PREVIEW_MAX_HEIGHT = 240;

function previewSize(attachment: AttachmentInfo) {
  const width = attachment.width ?? PREVIEW_MAX_WIDTH;
  const height = attachment.height ?? PREVIEW_MAX_HEIGHT;
  const scale = Math.min(1, PREVIEW_MAX_WIDTH / width, PREVIEW_MAX_HEIGHT / height);
  return { width: Math.round(width * scale), height: Math.round(height * scale) };
}

function typeLabel(attachment: AttachmentInfo): string {
  const extension = attachment.filename.includes('.') ? attachment.filename.split('.').pop()!.toUpperCase() : '';
  return extension && extension.length <= 5 ? `${formatSize(attachment.size_bytes)} · ${extension}` : formatSize(attachment.size_bytes);
}

function FileChip(props: { attachment: AttachmentInfo }) {
  return (
    <div class="file">
      <div class="fi"><Icon name={isImage(props.attachment) ? 'image' : 'file'} /></div>
      <div class="col grow" style={{ 'min-width': 0 }}>
        <span class="b trunc fs13" title={props.attachment.filename}>{props.attachment.filename}</span>
        <span class="t3 fs12">{typeLabel(props.attachment)}</span>
      </div>
      <Button iconOnly size="sm" variant="ghost" icon="dl" label={`Download ${props.attachment.filename}`} onClick={() => void downloadAttachment(props.attachment)} />
    </div>
  );
}

function ImageThumb(props: { attachment: AttachmentInfo; onOpen: () => void }) {
  const [url] = createResource(() => props.attachment, (attachment) => thumbUrl(attachment));
  const size = () => previewSize(props.attachment);
  return (
    <button type="button" class="img-thumb" style={{ width: `${size().width}px`, height: `${size().height}px` }}
      aria-label={`Open ${props.attachment.filename}`} onClick={props.onOpen}>
      <Show when={url()} fallback={<span class={url.error ? 'img-fail' : 'skel pulse'} style={{ width: '100%', height: '100%', 'border-radius': 0 }}>
        <Show when={url.error}><Icon name="warn" /></Show></span>}>
        <img src={url()} alt={props.attachment.filename} width={size().width} height={size().height} draggable={false} />
      </Show>
    </button>
  );
}

function ImageViewer(props: { attachment: AttachmentInfo; onClose: () => void }) {
  const [url] = createResource(() => props.attachment, (attachment) => imageUrl(attachment));
  return (
    <Modal open onClose={props.onClose} title={props.attachment.filename} width="min(92vw, 1100px)">
      <div class="col g12">
        <div class="img-viewer">
          <Show when={url()} fallback={url.error ? <span class="tr">The picture could not be loaded.</span> : <span class="spin" />}>
            <img src={url()} alt={props.attachment.filename} />
          </Show>
        </div>
        <div class="row g8" style={{ 'justify-content': 'flex-end' }}>
          <Button variant="secondary" icon="dl" onClick={() => void downloadAttachment(props.attachment)}>Download</Button>
          <Button variant="primary" onClick={props.onClose}>Close</Button>
        </div>
      </div>
    </Modal>
  );
}

/** Attachments of one message: pictures with a preview, everything else as a file chip. */
export function Attachments(props: { attachments: AttachmentInfo[] }) {
  const [opened, setOpened] = createSignal<AttachmentInfo | null>(null);
  return (
    <>
      <div class="row g12 wrap" style={{ 'align-items': 'flex-end' }}>
        <For each={props.attachments}>
          {(attachment) => (
            <Show when={isImage(attachment) && attachment.has_thumb} fallback={<FileChip attachment={attachment} />}>
              <ImageThumb attachment={attachment} onOpen={() => setOpened(attachment)} />
            </Show>
          )}
        </For>
      </div>
      <Show when={opened()}>{(attachment) => <ImageViewer attachment={attachment()} onClose={() => setOpened(null)} />}</Show>
    </>
  );
}
