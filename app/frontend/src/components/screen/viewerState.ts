// The state of the screen viewer as the person sees it (design E4), kept apart from the video
// decoder so it can be tested with plain numbers: the component feeds it events and a clock.

export type EndReason = 'stopped' | 'left' | 'disconnected' | 'room-closed';

export type ViewerStatus =
  | 'connecting' // asked for the stream, no picture yet
  | 'live' // pictures are arriving
  | 'buffering' // they stopped arriving for a while
  | 'weak' // arriving, but slowly: the connection is weak
  | 'decoder-reset' // the decoder failed and was restarted
  | 'reconnecting' // our own connection to the server is down
  | 'unsupported' // this computer cannot decode H.264
  | 'ended';

export interface ViewerSnapshot {
  status: ViewerStatus;
  endReason: EndReason | null;
  fps: number;
  /** Received video in kilobits per second over the last second. */
  bitrateKbps: number;
  width: number;
  height: number;
  /** The component should ask the sharer for a fresh key frame now. */
  wantKeyFrame: boolean;
}

/** No picture for this long, after having had one, counts as buffering. */
export const STALL_MS = 1500;
/** Below this many frames per second for two seconds in a row the connection is called weak. */
export const WEAK_FPS = 20;
/** While waiting for a picture a key frame is asked for this often. */
export const KEYFRAME_RETRY_MS = 1000;
/** The sharer stopped: the "ended" screen closes itself after this long. */
export const ENDED_AUTOCLOSE_MS = 10_000;

export class ViewerMachine {
  private status: ViewerStatus = 'connecting';
  private endReason: EndReason | null = null;
  private width = 0;
  private height = 0;
  private framesInWindow = 0;
  private bytesInWindow = 0;
  private windowStart: number;
  private fps = 0;
  private bitrateKbps = 0;
  private lastFrameAt: number | null = null;
  private lastKeyAsk: number | null = null;
  private slowSeconds = 0;
  private offline = false;
  private endedAt: number | null = null;
  private wantKey = false;

  constructor(now: number) {
    this.windowStart = now;
    this.lastKeyAsk = now;
  }

  /** Bytes of a video chunk arrived (before decoding). */
  chunk(bytes: number) {
    this.bytesInWindow += bytes;
  }

  /** A picture was drawn. */
  frame(width: number, height: number, now: number) {
    if (this.status === 'ended' || this.status === 'unsupported') return;
    this.width = width;
    this.height = height;
    this.framesInWindow += 1;
    this.lastFrameAt = now;
    if (this.status === 'connecting' || this.status === 'buffering' || this.status === 'decoder-reset') this.status = this.offline ? 'reconnecting' : 'live';
  }

  /** The decoder failed and was recreated; deltas are useless until the next key frame. */
  decoderReset(now: number) {
    if (this.status === 'ended' || this.status === 'unsupported') return;
    this.status = 'decoder-reset';
    this.wantKey = true;
    this.lastKeyAsk = now;
  }

  unsupported() {
    this.status = 'unsupported';
  }

  /** Our connection to the server went down (`true`) or came back (`false`). */
  connection(offline: boolean, now: number) {
    this.offline = offline;
    if (this.status === 'ended' || this.status === 'unsupported') return;
    if (offline) this.status = 'reconnecting';
    else {
      // Back online: the stream restarts with a key frame.
      this.status = 'connecting';
      this.lastFrameAt = null;
      this.wantKey = true;
      this.lastKeyAsk = now;
    }
  }

  ended(reason: EndReason, now: number) {
    this.status = 'ended';
    this.endReason = reason;
    this.endedAt = now;
  }

  /** `true` once an ended screen has been up long enough to close by itself. */
  shouldAutoClose(now: number): boolean {
    return this.status === 'ended' && this.endedAt !== null && now - this.endedAt >= ENDED_AUTOCLOSE_MS;
  }

  /** Advance the clock (call about twice a second). */
  tick(now: number): ViewerSnapshot {
    if (now - this.windowStart >= 1000) {
      const seconds = (now - this.windowStart) / 1000;
      this.fps = Math.round(this.framesInWindow / seconds);
      this.bitrateKbps = Math.round((this.bytesInWindow * 8) / 1000 / seconds);
      this.slowSeconds = this.fps > 0 && this.fps < WEAK_FPS ? this.slowSeconds + 1 : 0;
      this.framesInWindow = 0;
      this.bytesInWindow = 0;
      this.windowStart = now;
    }

    const terminal = this.status === 'ended' || this.status === 'unsupported' || this.status === 'reconnecting';
    if (!terminal) {
      if (this.status === 'live' || this.status === 'weak') {
        if (this.lastFrameAt !== null && now - this.lastFrameAt >= STALL_MS) {
          this.status = 'buffering';
          this.wantKey = true;
          this.lastKeyAsk = now;
        } else {
          this.status = this.slowSeconds >= 2 ? 'weak' : 'live';
        }
      }
      // Still waiting (for the first picture, or after a stall): keep asking.
      if ((this.status === 'connecting' || this.status === 'buffering' || this.status === 'decoder-reset')
        && this.lastKeyAsk !== null && now - this.lastKeyAsk >= KEYFRAME_RETRY_MS) {
        this.wantKey = true;
        this.lastKeyAsk = now;
      }
    }

    const wantKeyFrame = this.wantKey;
    this.wantKey = false;
    return {
      status: this.status,
      endReason: this.endReason,
      fps: this.status === 'live' || this.status === 'weak' ? this.fps : 0,
      bitrateKbps: this.status === 'live' || this.status === 'weak' ? this.bitrateKbps : 0,
      width: this.width,
      height: this.height,
      wantKeyFrame,
    };
  }
}

/** Why a share ended, from the server's word for it. */
export function endReasonFromServer(reason: string | undefined): EndReason {
  switch (reason) {
    case 'Left': return 'left';
    case 'Disconnected': return 'disconnected';
    case 'RoomClosed': return 'room-closed';
    default: return 'stopped';
  }
}
