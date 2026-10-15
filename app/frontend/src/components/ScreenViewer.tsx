import { onMount, onCleanup, createSignal } from 'solid-js';
import { Channel } from '@tauri-apps/api/core';
import { invoke } from '@tauri-apps/api/core';

interface VideoChunkPayload {
  is_key: boolean;
  data: number[];
}

export function ScreenViewer() {
  let canvasRef: HTMLCanvasElement | undefined;
  let decoder: VideoDecoder | null = null;
  const [fps, setFps] = createSignal<number>(0);
  let frameCounter = 0;
  let lastFpsTime = performance.now();

  onMount(() => {
    if (!canvasRef) return;
    const ctx = canvasRef.getContext('2d');
    if (!ctx) return;

    if (typeof (window as any).VideoDecoder === 'undefined') {
      console.warn('WebCodecs VideoDecoder API not supported in this Webview');
      return;
    }

    try {
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
        error: (e: any) => {
          console.error('WebCodecs VideoDecoder error:', e);
        },
      });

      decoder.configure({
        codec: 'avc1.42E01E', // Baseline profile level 3.0
        optimizeForLatency: true,
      });

      // Setup Tauri IPC Channel for NAL fragments
      const onFrameChannel = new Channel<VideoChunkPayload>();
      let microTimestamp = 0;

      onFrameChannel.onmessage = (payload: VideoChunkPayload) => {
        if (!decoder || decoder.state !== 'configured') return;

        try {
          const chunkData = new Uint8Array(payload.data);
          const chunk = new EncodedVideoChunk({
            type: payload.is_key ? 'key' : 'delta',
            timestamp: microTimestamp,
            data: chunkData,
          });
          microTimestamp += 16666; // ~60 fps
          decoder.decode(chunk);
        } catch (err) {
          console.error('Failed to decode video chunk:', err);
        }
      };

      // Register the channel with Rust backend
      invoke('register_video_channel', { onFrame: onFrameChannel }).catch(console.error);
    } catch (err) {
      console.error('Failed to initialize VideoDecoder:', err);
    }
  });

  onCleanup(() => {
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
        GPU WebCodecs {fps()} FPS
      </div>
    </div>
  );
}
