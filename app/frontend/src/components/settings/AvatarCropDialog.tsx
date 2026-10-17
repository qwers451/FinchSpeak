import { createSignal, onCleanup, onMount, Show } from 'solid-js';
import { saveAvatar } from '../../stores/profileStore';
import { Button, Icon, Modal, Slider } from '../../ui';
import { t } from '../../i18n';

/** The side of the square that is cut out and sent (the server makes it 128 px). */
export const CROP_SIDE = 256;
/** Pictures bigger than this are refused before they are even read. */
export const MAX_SOURCE_BYTES = 12 * 1024 * 1024;
const ACCEPTED = ['image/png', 'image/jpeg', 'image/webp'];
type AvatarError = { kind: 'local'; source: string } | { kind: 'external'; text: string };

/** Why a chosen file cannot be used, or `null`. */
export function checkAvatarFile(file: { type: string; size: number }): string | null {
  if (!ACCEPTED.includes(file.type)) return 'Use a PNG, JPEG or WebP picture';
  if (file.size > MAX_SOURCE_BYTES) return 'That picture is too large. Pick one under 12 MB';
  return null;
}

/** Where the picture sits in the square for a given zoom and pan (pan in square pixels, clamped so no gap shows). */
export function placement(imageWidth: number, imageHeight: number, zoom: number, panX: number, panY: number) {
  const base = CROP_SIDE / Math.min(imageWidth, imageHeight);
  const scale = base * Math.max(1, zoom);
  const width = imageWidth * scale;
  const height = imageHeight * scale;
  const limitX = Math.max(0, (width - CROP_SIDE) / 2);
  const limitY = Math.max(0, (height - CROP_SIDE) / 2);
  const x = Math.min(limitX, Math.max(-limitX, panX));
  const y = Math.min(limitY, Math.max(-limitY, panY));
  return { width, height, left: (CROP_SIDE - width) / 2 + x, top: (CROP_SIDE - height) / 2 + y, panX: x, panY: y };
}

/** F2: place the picture inside the circle (drag to move, slider to zoom), then upload. */
export function AvatarCropDialog(props: { file: File; onClose: () => void; onDone: () => void }) {
  let canvas: HTMLCanvasElement | undefined;
  let image: ImageBitmap | undefined;
  const [zoom, setZoom] = createSignal(1);
  const [pan, setPan] = createSignal({ x: 0, y: 0 });
  const [error, setError] = createSignal<AvatarError | null>(null);
  const setLocalError = (source: string) => setError({ kind: 'local', source });
  const errorText = () => {
    const current = error();
    if (!current) return '';
    return current.kind === 'local' ? t(current.source) : current.text;
  };
  const [busy, setBusy] = createSignal(false);
  const [ready, setReady] = createSignal(false);
  let drag: { x: number; y: number; panX: number; panY: number } | null = null;

  const draw = () => {
    const ctx = canvas?.getContext('2d');
    if (!ctx || !image) return;
    const p = placement(image.width, image.height, zoom(), pan().x, pan().y);
    ctx.clearRect(0, 0, CROP_SIDE, CROP_SIDE);
    ctx.drawImage(image, p.left, p.top, p.width, p.height);
  };

  onMount(async () => {
    try {
      image = await createImageBitmap(props.file);
      setReady(true);
      draw();
    } catch {
      setLocalError('That picture could not be opened. Try another file');
    }
  });
  onCleanup(() => image?.close());

  const setPanClamped = (x: number, y: number) => {
    if (!image) return;
    const p = placement(image.width, image.height, zoom(), x, y);
    setPan({ x: p.panX, y: p.panY });
    draw();
  };

  const onDown = (event: PointerEvent) => {
    drag = { x: event.clientX, y: event.clientY, panX: pan().x, panY: pan().y };
    (event.currentTarget as HTMLElement).setPointerCapture?.(event.pointerId);
  };
  const onMove = (event: PointerEvent) => {
    if (drag) setPanClamped(drag.panX + event.clientX - drag.x, drag.panY + event.clientY - drag.y);
  };
  const onUp = () => { drag = null; };

  const upload = async () => {
    if (!canvas || busy()) return;
    setBusy(true);
    setError(null);
    const blob = await new Promise<Blob | null>((resolve) => canvas!.toBlob(resolve, 'image/png'));
    if (!blob) {
      setBusy(false);
      setLocalError('The picture could not be prepared');
      return;
    }
    const failure = await saveAvatar(new Uint8Array(await blob.arrayBuffer()), 'image/png');
    setBusy(false);
    if (failure?.kind === 'local') setLocalError(failure.message);
    else if (failure) setError({ kind: 'external', text: failure.message });
    else props.onDone();
  };

  return (
    <Modal open onClose={props.onClose} title={t('Position your picture')} width="420px">
      <div class="col g16 center">
        <div class="crop-frame" onPointerDown={onDown} onPointerMove={onMove} onPointerUp={onUp} onPointerCancel={onUp} role="img" aria-label={t('Preview of your picture')}>
          <canvas ref={canvas} width={CROP_SIDE} height={CROP_SIDE} />
          <div class="crop-ring" />
          <Show when={!ready() && !error()}><span class="spin" style={{ position: 'absolute' }} /></Show>
        </div>
        <div class="row g12" style={{ width: '100%' }}>
          <Icon name="image" size="sm" class="t3" />
          <div class="grow">
            <Slider label={t('Zoom')} min={1} max={3} step={0.05} value={zoom()} onChange={(value) => { setZoom(value); setPanClamped(pan().x, pan().y); }} />
          </div>
        </div>
        <span class="t3 fs12">{t('Drag the picture to move it. Everyone sees the part inside the circle.')}</span>
        <Show when={error()}>
          <div class="auth-banner red" role="alert" style={{ width: '100%' }}><Icon name="warn" class="tr" /><span class="fs13">{errorText()}</span></div>
        </Show>
        <div class="row g8" style={{ 'justify-content': 'flex-end', width: '100%' }}>
          <Button variant="secondary" onClick={props.onClose}>{t('Cancel')}</Button>
          <Button variant="primary" loading={busy()} disabled={!ready()} onClick={() => void upload()}>{t('Use this picture')}</Button>
        </div>
      </div>
    </Modal>
  );
}
