import { onMount, onCleanup, createSignal } from 'solid-js';
import { Channel, invoke } from '@tauri-apps/api/core';

interface VideoChunkPayload {
  is_key: boolean;
  data: number[];
}

// Constrained Baseline, level 5.1: OpenH264 picks the real level itself and the SPS in the
// stream is authoritative, but a low level here makes some decoders refuse 1080p and above.
const CODEC = 'avc1.42C033';

export function ScreenViewer(props: { peerId: number }) {
  let canvasRef: HTMLCanvasElement | undefined;
  let decoder: VideoDecoder | null = null;
  let closed = false;
  /** Deltas are useless until a key frame has been given to the decoder (start, or after an error). */
  let waitingForKey = true;
  const [fps, setFps] = createSignal<number>(0);
  const [unsupported, setUnsupported] = createSignal(false);
  let frameCounter = 0;
  let lastFpsTime = performance.now();
  let microTimestamp = 0;

  const askForKeyFrame = () => {
    invoke('request_idr_keyframe', { targetPeerId: props.peerId }).catch(console.error);
  };

  const createDecoder = () => {
    const ctx = canvasRef?.getContext('2d');
    if (!ctx) return;
    decoder = new VideoDecoder({
      output: (frame: VideoFrame) => {
        if (canvasRef) {
          canvasRef.width = frame.displayWidth;
          canvasRef.height = frame.displayHeight;
          ctx.drawImage(frame, 0, 0, frame.displayWidth, frame.displayHeight);
        }
        frame.close();

        frameCounter++;
        const now = performance.now();
        if (now - lastFpsTime >= 1000) {
          setFps(Math.round((frameCounter * 1000) / (now - lastFpsTime)));
          frameCounter = 0;
          lastFpsTime = now;
        }
      },
      error: (e: unknown) => {
        // A broken stream closes the decoder for good: start over and ask for a key frame.
        console.error('WebCodecs VideoDecoder error:', e);
        if (closed) return;
        waitingForKey = true;
        createDecoder();
        askForKeyFrame();
      },
    });
    decoder.configure({ codec: CODEC, optimizeForLatency: true });
  };

  onMount(() => {
    if (!canvasRef) return;
    if (typeof (window as unknown as { VideoDecoder?: unknown }).VideoDecoder === 'undefined') {
      console.warn('WebCodecs VideoDecoder API not supported in this Webview');
      setUnsupported(true);
      return;
    }

    try {
      createDecoder();

      const onFrameChannel = new Channel<VideoChunkPayload>();
      onFrameChannel.onmessage = (payload: VideoChunkPayload) => {
        if (!decoder || decoder.state !== 'configured') return;
        if (waitingForKey && !payload.is_key) return;
        try {
          decoder.decode(new EncodedVideoChunk({
            type: payload.is_key ? 'key' : 'delta',
            timestamp: microTimestamp,
            data: new Uint8Array(payload.data),
          }));
          waitingForKey = false;
          microTimestamp += 33333; // ~30 fps; only has to increase
        } catch (err) {
          console.error('Failed to decode video chunk:', err);
          waitingForKey = true;
          askForKeyFrame();
        }
      };

      invoke('register_video_channel', { onFrame: onFrameChannel }).catch(console.error);
    } catch (err) {
      console.error('Failed to initialize VideoDecoder:', err);
      setUnsupported(true);
    }
  });

  onCleanup(() => {
    closed = true;
    if (decoder && decoder.state !== 'closed') {
      decoder.close();
    }
  });

  return (
    <div style={{ position: "relative", width: "100%", height: "100%", background: "#000", display: "flex", "align-items": "center", "justify-content": "center" }}>
      <canvas ref={canvasRef} style={{ "max-width": "100%", "max-height": "100%", "object-fit": "contain" }} />
      <div style={{
        position: "absolute",
        top: "12px",
        left: "12px",
        background: "rgba(0, 0, 0, 0.7)",
        color: "var(--accent-teal)",
        padding: "4px 8px",
        "border-radius": "4px",
        "font-size": "0.78rem",
        "font-family": "monospace"
      }}>
        {unsupported() ? 'Video playback is not supported here' : `${fps()} FPS`}
      </div>
    </div>
  );
}
