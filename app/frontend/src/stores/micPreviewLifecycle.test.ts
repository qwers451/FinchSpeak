import { expect, it } from 'vitest';
import { handleMicLevel, handleMicTest, micActivation, micLevelDb, micTestPhase, MIC_FLOOR_DB, resetMicPreview } from './voiceStore';
import { resetSessionState, restoreSessionSnapshot, sessionHandlers } from './sessionStore';

const seedMicPreview = () => {
 handleMicLevel({ db: -18, speaking: true });
 handleMicTest({ phase: 'recording' });
 expect(micActivation()).toBe(true);
 expect(micTestPhase()).toBe('recording');
};

const expectMicPreviewClear = () => {
 expect(micActivation()).toBe(false);
 expect(micLevelDb()).toBe(MIC_FLOOR_DB);
 expect(micTestPhase()).toBe('idle');
};

it('clears stale microphone level, activation and test state when the device/session is lost', () => {
 seedMicPreview();
 resetMicPreview();
 expectMicPreviewClear();
});

it.each([
 ['full session reset', () => resetSessionState()],
 ['lost connection', () => sessionHandlers['connection-lost']('network unavailable')],
 ['lost audio device', () => sessionHandlers['audio-device-lost'](null)],
 ['failed audio device', () => sessionHandlers['audio-device-failed'](null)],
 ['disconnected restored snapshot', () => restoreSessionSnapshot({ connected: false, user_id: '', session_peer_id: 0, current_voice_channel: null, muted: false, deafened: false, pending_tofu: null, initial_state: null })],
])('clears microphone preview after %s', (_label, transition) => {
 seedMicPreview();
 transition();
 expectMicPreviewClear();
});

it('clears local activation and marks the level inactive when audio processing fails', () => {
 seedMicPreview();
 sessionHandlers['audio-processing-failed']({ message: 'capture failed' });
 expect(micActivation()).toBe(false);
 expect(micLevelDb()).toBeLessThanOrEqual(MIC_FLOOR_DB);
 expect(micTestPhase()).toBe('idle');
});
