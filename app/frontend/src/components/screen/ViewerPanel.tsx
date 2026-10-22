import { Channel, invoke } from '@tauri-apps/api/core';
import { createEffect, createMemo, createSignal, onCleanup, onMount, Show } from 'solid-js';
import { closeEndedShare, endedShare, stopWatching, watching } from '../../stores/screenStore';
import { connectionStatus, knownUsers } from '../../stores/sessionStore';
import { allVoiceParticipants } from '../../stores/voiceStore';
import { screenAudioKey, setVolume, volumeFor } from '../../stores/volumeStore';
import { Button, Icon, Slider, pushToast } from '../../ui';
import { ENDED_AUTOCLOSE_MS, ViewerMachine, type ViewerSnapshot } from './viewerState';
import { formatBitrate, t, unit } from '../../i18n';

interface VideoChunkPayload {
  is_key: boolean;
  data: number[];
}

// Constrained Baseline, level 5.1: OpenH264 picks the real level itself and the SPS in the
// stream is authoritative, but a low level here makes some decoders refuse 1080p and above.
const CODEC = 'avc1.42C033';
/** The HUD fades out after this long without the mouse moving. */
const HUD_HIDE_MS = 3000;

const INITIAL: ViewerSnapshot = { status: 'connecting', endReason: null, fps: 0, bitrateKbps: 0, width: 0, height: 0, wantKeyFrame: false };

/** E3/E4: the picture of the share you watch, with HUD, volume, full screen and every state of the stream. */
export function ViewerPanel() {
  const share = () => watching();
  return <Show when={share()}>{(current) => <Viewer peerId={current().peer_id} userId={current().user_id} />}</Show>;
}

function Viewer(props: { peerId: number; userId: string }) {
  let canvas: HTMLCanvasElement | undefined;
  let frame: HTMLDivElement | undefined;
  let decoder: VideoDecoder | null = null;
  let closed = false;
  let waitingForKey = true;
  let microTimestamp = 0;
  const machine = new ViewerMachine(performance.now());
  const [view, setView] = createSignal<ViewerSnapshot>(INITIAL);
  const [hud, setHud] = createSignal(true);
  const [fullscreen, setFullscreen] = createSignal(false);
  let hudTimer: ReturnType<typeof setTimeout> | undefined;

  const name = createMemo(() => {
    const person = allVoiceParticipants().find((participant) => participant.peer_id === props.peerId);
    return knownUsers().find((user) => user.id === props.userId)?.display_name ?? person?.username ?? t('Someone');
  });
  const volumeKey = () => `screen:${props.userId}`;
  const pipSupported = () => typeof document !== 'undefined' && 'pictureInPictureEnabled' in document && document.pictureInPictureEnabled;
  const fullscreenSupported = () => typeof document !== 'undefined' && document.fullscreenEnabled !== false;

  const askForKeyFrame = () => { invoke('request_idr_keyframe', { targetPeerId: props.peerId }).catch(console.error); };

  const createDecoder = () => {
    const ctx = canvas?.getContext('2d');
    if (!ctx) return;
    decoder = new VideoDecoder({
      output: (picture: VideoFrame) => {
        if (canvas) {
          if (canvas.width !== picture.displayWidth || canvas.height !== picture.displayHeight) {
            canvas.width = picture.displayWidth;
            canvas.height = picture.displayHeight;
          }
          ctx.drawImage(picture, 0, 0, picture.displayWidth, picture.displayHeight);
        }
        machine.frame(picture.displayWidth, picture.displayHeight, performance.now());
        picture.close();
      },
      error: (e: unknown) => {
        // A broken stream closes the decoder for good: start over and ask for a key frame.
        console.error('WebCodecs VideoDecoder error:', e);
        if (closed) return;
        waitingForKey = true;
        machine.decoderReset(performance.now());
        createDecoder();
        askForKeyFrame();
      },
    });
    decoder.configure({ codec: CODEC, optimizeForLatency: true });
  };

  const showHud = () => {
    setHud(true);
    clearTimeout(hudTimer);
    hudTimer = setTimeout(() => setHud(false), HUD_HIDE_MS);
  };

  onMount(async () => {
    showHud();
    const tick = setInterval(() => {
      const now = performance.now();
      const snapshot = machine.tick(now);
      setView(snapshot);
      if (snapshot.wantKeyFrame) { waitingForKey = true; askForKeyFrame(); }
    }, 500);
    onCleanup(() => clearInterval(tick));

    const onFullscreen = () => setFullscreen(document.fullscreenElement === frame);
    document.addEventListener('fullscreenchange', onFullscreen);
    onCleanup(() => document.removeEventListener('fullscreenchange', onFullscreen));

    if (typeof (window as unknown as { VideoDecoder?: unknown }).VideoDecoder === 'undefined') {
      machine.unsupported();
      setView(machine.tick(performance.now()));
      return;
    }
    try {
      const support = await VideoDecoder.isConfigSupported({ codec: CODEC, optimizeForLatency: true });
      if (!support.supported) throw new Error('H.264 is not supported');
      createDecoder();
      const onFrameChannel = new Channel<VideoChunkPayload>();
      onFrameChannel.onmessage = (payload: VideoChunkPayload) => {
        machine.chunk(payload.data.length);
        if (!decoder || decoder.state !== 'configured') return;
        if (waitingForKey && !payload.is_key) return;
        try {
          decoder.decode(new EncodedVideoChunk({ type: payload.is_key ? 'key' : 'delta', timestamp: microTimestamp, data: new Uint8Array(payload.data) }));
          waitingForKey = false;
          microTimestamp += 33333; // ~30 fps; only has to increase
        } catch (err) {
          console.error('Failed to decode video chunk:', err);
          waitingForKey = true;
          machine.decoderReset(performance.now());
          askForKeyFrame();
        }
      };
      await invoke('register_video_channel', { onFrame: onFrameChannel });
    } catch (err) {
      console.error('Failed to initialize VideoDecoder:', err);
      machine.unsupported();
      setView(machine.tick(performance.now()));
    }
  });

  // Our own connection dropping and coming back is part of the picture's story.
  const offline = () => connectionStatus() !== 'connected';
  let wasOffline = false;
  createEffect(() => {
    const now = offline();
    if (now !== wasOffline) {
      wasOffline = now;
      machine.connection(now, performance.now());
    }
  });

  onCleanup(() => {
    closed = true;
    clearTimeout(hudTimer);
    if (decoder && decoder.state !== 'closed') decoder.close();
    if (document.fullscreenElement === frame) void document.exitFullscreen();
  });

  const toggleFullscreen = async () => {
    try {
      if (document.fullscreenElement) await document.exitFullscreen();
      else await frame?.requestFullscreen();
    } catch (err) {
      pushToast({ kind: 'warn', title: t('Full screen is not available'), text: String(err) });
    }
  };

  const pictureInPicture = async () => {
    if (!canvas) return;
    try {
      if (document.pictureInPictureElement) { await document.exitPictureInPicture(); return; }
      const video = document.createElement('video');
      video.muted = true;
      video.srcObject = canvas.captureStream(30);
      await video.play();
      await video.requestPictureInPicture();
      video.addEventListener('leavepictureinpicture', () => { video.srcObject = null; }, { once: true });
    } catch (err) {
      pushToast({ kind: 'warn', title: t('Picture-in-picture is not available'), text: String(err) });
    }
  };

  const copyDetails = () => {
    void navigator.clipboard?.writeText(`FinchSpeak viewer\ncodec: ${CODEC}\nVideoDecoder: ${typeof (window as unknown as { VideoDecoder?: unknown }).VideoDecoder}\nagent: ${navigator.userAgent}`);
    pushToast({ kind: 'success', title: t('Details copied') });
  };

  const status = () => view().status;
  const showOverlay = () => ['connecting', 'buffering', 'decoder-reset', 'reconnecting', 'unsupported'].includes(status());
  const overlay = () => ({
    connecting: { head: 'Waiting for the first picture…', body: t("Asking {name}'s computer for a full frame. Usually under a second.", { name: name() }), spin: true },
    buffering: { head: 'Catching up…', body: 'Some packets were lost; waiting for a clean frame.', spin: true },
    'decoder-reset': { head: 'Restoring the picture…', body: 'A frame could not be decoded; asking for a fresh one.', spin: true },
    reconnecting: { head: 'Reconnecting…', body: 'The picture resumes automatically when you are back.', spin: true },
    unsupported: { head: 'This computer cannot play the video', body: 'Its graphics driver has no H.264 decoder. Voice still works. Update the graphics driver, or ask for a window share at lower quality.', spin: false },
  } as Record<string, { head: string; body: string; spin: boolean }>)[status()];

  return (
    <div ref={frame} class={`viewer${fullscreen() ? ' is-fullscreen' : ''}`} onMouseMove={showHud} role="region" aria-label={t("{name}'s shared screen", { name: name() })}>
      <canvas ref={canvas} class="viewer-canvas" />

      <Show when={showOverlay() && overlay()}>
        {(o) => (
          <div class={`viewer-overlay${status() === 'unsupported' ? ' bad' : ''}`} role="status">
            <Show when={o().spin}><span class="spin" /></Show>
            <strong class="head" style={{ 'font-size': '17px' }}>{t(o().head)}</strong>
            <span class="t2 fs12" style={{ 'max-width': '300px' }}>{t(o().body)}</span>
            <Show when={status() === 'unsupported'}><Button size="sm" variant="secondary" icon="copy" onClick={copyDetails}>{t('Copy details')}</Button></Show>
          </div>
        )}
      </Show>

      <div class={`viewer-hud top${hud() || showOverlay() || status() === 'weak' ? ' on' : ''}`}>
        <span class="tag cyan">{name()}</span>
        <Show when={view().width > 0}><span class="tag cyan">{view().width} × {view().height}</span></Show>
        <Show when={status() === 'live' || status() === 'weak'}><span class="tag cyan">{view().fps} {unit('fps')} · {formatBitrate(view().bitrateKbps)}</span></Show>
        <Show when={status() === 'weak'}><span class="tag amber">{t('Weak connection')}</span></Show>
      </div>

      <div class={`viewer-hud bottom${hud() ? ' on' : ''}`}>
        <Icon name={volumeFor(volumeKey()) === 0 ? 'vol-off' : 'vol'} size="sm" />
        <div style={{ width: '120px' }}>
          <Slider cool label={t('Volume of the shared sound')} min={0} max={2} step={0.05} value={volumeFor(volumeKey())} onChange={(value) => void setVolume(volumeKey(), screenAudioKey(props.peerId), value)} />
        </div>
        <span class="grow" />
        <Show when={pipSupported()}><Button iconOnly size="sm" variant="ghost" icon="pip" label={t('Picture in picture')} onClick={() => void pictureInPicture()} /></Show>
        <Show when={fullscreenSupported()}><Button iconOnly size="sm" variant="ghost" icon="full" label={fullscreen() ? t('Leave full screen') : t('Full screen')} onClick={() => void toggleFullscreen()} /></Show>
        <Button size="sm" variant="secondary" icon="x" onClick={() => void stopWatching()}>{t('Stop watching')}</Button>
      </div>
    </div>
  );
}

/** E4: the share you watched has ended; says why, and closes by itself after ten seconds. */
export function EndedPanel() {
  const reason = () => endedShare();
  return <Show when={reason()}>{(ended) => <Ended name={ended().name} reason={ended().reason} />}</Show>;
}

function Ended(props: { name: string; reason: 'stopped' | 'left' | 'disconnected' | 'room-closed' }) {
  const text = () => ({
    stopped: { head: t('{name} stopped sharing', { name: props.name }), body: t('The room is still going. You can keep talking.') },
    left: { head: t('{name} left the room', { name: props.name }), body: t('Their screen share ended with the call.') },
    disconnected: { head: t('{name} lost the connection', { name: props.name }), body: t('Their screen share ended. It starts again if they come back and share.') },
    'room-closed': { head: t('The voice room was closed'), body: t('An admin deleted this room, so the screen share ended.') },
  })[props.reason];
  const timer = setTimeout(closeEndedShare, ENDED_AUTOCLOSE_MS);
  onCleanup(() => clearTimeout(timer));
  return (
    <div class="viewer ended" role="status">
      <div class="viewer-overlay" style={{ background: 'rgba(5, 8, 15, .9)' }}>
        <strong class="head" style={{ 'font-size': '17px' }}>{text().head}</strong>
        <span class="t2 fs12">{text().body}</span>
        <Button variant="primary" size="sm" onClick={closeEndedShare}>{t('Back to the room')}</Button>
      </div>
    </div>
  );
}
