import { describe, expect, it } from 'vitest';
import { ENDED_AUTOCLOSE_MS, endReasonFromServer, KEYFRAME_RETRY_MS, STALL_MS, ViewerMachine } from './viewerState';

/** Feed `fps` frames in each of `seconds` seconds, ticking every 500 ms; returns the last snapshot. */
function run(machine: ViewerMachine, from: number, seconds: number, fps: number, bytesPerFrame = 12_500) {
  let now = from;
  let snapshot = machine.tick(now);
  for (let half = 0; half < seconds * 2; half++) {
    for (let f = 0; f < fps / 2; f++) {
      machine.chunk(bytesPerFrame);
      machine.frame(1920, 1080, now + 1);
    }
    now += 500;
    snapshot = machine.tick(now);
  }
  return { snapshot, now };
}

describe('ViewerMachine', () => {
  it('starts as connecting and asks for a key frame every second until a picture arrives', () => {
    const machine = new ViewerMachine(0);
    expect(machine.tick(500)).toMatchObject({ status: 'connecting', wantKeyFrame: false });
    expect(machine.tick(KEYFRAME_RETRY_MS).wantKeyFrame).toBe(true);
    expect(machine.tick(KEYFRAME_RETRY_MS + 200).wantKeyFrame).toBe(false);
    expect(machine.tick(2 * KEYFRAME_RETRY_MS + 100).wantKeyFrame).toBe(true);
  });

  it('is live with stats once pictures arrive', () => {
    const machine = new ViewerMachine(0);
    const { snapshot } = run(machine, 0, 3, 30);
    expect(snapshot).toMatchObject({ status: 'live', fps: 30, width: 1920, height: 1080 });
    expect(snapshot.bitrateKbps).toBe(3000); // 30 fps x 12.5 kB x 8
  });

  it('shows buffering when pictures stop, and recovers when they resume', () => {
    const machine = new ViewerMachine(0);
    const { now } = run(machine, 0, 2, 30);
    const stalled = machine.tick(now + STALL_MS + 100);
    expect(stalled.status).toBe('buffering');
    expect(stalled.wantKeyFrame).toBe(true);
    machine.frame(1920, 1080, now + STALL_MS + 200);
    expect(machine.tick(now + STALL_MS + 300).status).toBe('live');
  });

  it('calls a slow stream weak only after two slow seconds, and live again once it speeds up', () => {
    const machine = new ViewerMachine(0);
    let { snapshot, now } = run(machine, 0, 1, 30);
    ({ snapshot, now } = run(machine, now, 1, 12));
    expect(snapshot.status).toBe('live');
    ({ snapshot, now } = run(machine, now, 2, 12));
    expect(snapshot.status).toBe('weak');
    ({ snapshot } = run(machine, now, 2, 30));
    expect(snapshot.status).toBe('live');
  });

  it('a decoder reset asks for a fresh key frame and shows restoring until a picture is drawn', () => {
    const machine = new ViewerMachine(0);
    const { now } = run(machine, 0, 2, 30);
    machine.decoderReset(now);
    const snapshot = machine.tick(now + 100);
    expect(snapshot).toMatchObject({ status: 'decoder-reset', wantKeyFrame: true });
    machine.frame(1280, 720, now + 200);
    expect(machine.tick(now + 300)).toMatchObject({ status: 'live', width: 1280 });
  });

  it('says reconnecting while we are offline, then starts over with a key frame', () => {
    const machine = new ViewerMachine(0);
    const { now } = run(machine, 0, 2, 30);
    machine.connection(true, now);
    expect(machine.tick(now + 3000).status).toBe('reconnecting');
    machine.connection(false, now + 4000);
    const back = machine.tick(now + 4100);
    expect(back).toMatchObject({ status: 'connecting', wantKeyFrame: true });
    machine.frame(1920, 1080, now + 4200);
    expect(machine.tick(now + 4300).status).toBe('live');
  });

  it('ended is final: later pictures or connection changes do not bring the player back', () => {
    const machine = new ViewerMachine(0);
    machine.ended('left', 100);
    machine.frame(1920, 1080, 200);
    machine.connection(true, 300);
    machine.connection(false, 400);
    expect(machine.tick(500)).toMatchObject({ status: 'ended', endReason: 'left' });
  });

  it('closes an ended screen by itself after ten seconds', () => {
    const machine = new ViewerMachine(0);
    machine.ended('stopped', 1000);
    expect(machine.shouldAutoClose(1000 + ENDED_AUTOCLOSE_MS - 1)).toBe(false);
    expect(machine.shouldAutoClose(1000 + ENDED_AUTOCLOSE_MS)).toBe(true);
    expect(new ViewerMachine(0).shouldAutoClose(999_999)).toBe(false);
  });

  it('unsupported stays unsupported', () => {
    const machine = new ViewerMachine(0);
    machine.unsupported();
    machine.frame(1, 1, 1);
    machine.decoderReset(2);
    expect(machine.tick(3).status).toBe('unsupported');
  });

  it('maps the server\'s words for why a share ended', () => {
    expect(endReasonFromServer('Left')).toBe('left');
    expect(endReasonFromServer('Disconnected')).toBe('disconnected');
    expect(endReasonFromServer('RoomClosed')).toBe('room-closed');
    expect(endReasonFromServer('Stopped')).toBe('stopped');
    expect(endReasonFromServer(undefined)).toBe('stopped');
  });
});
