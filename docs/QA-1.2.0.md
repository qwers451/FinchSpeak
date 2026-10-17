# FinchSpeak 1.2.0 — QA and release evidence

Status: implementation and local verification; stable release is not published yet. Updated 18 October 2026 (Europe/Moscow). Baseline: main9f500ce / published v1.1.0 source807a893. Product scope and deliberate omissions are in docs/superpowers/specs/2026-10-17-desktop-1.2.0-design.md.

## Reproduced v1.1.0 capture regression
WASAPI can report equal callback instants for adjacent10ms buffers while capture positions advance. v1.1.0 CaptureClock rejected callback<=previous_callback. Alternating tagged/untagged buffers left no complete fresh20ms resampler input eligible for Opus. Consequently no outbound voice and no local speaking indication, even with a working microphone.

An explicit ignored hardware test captures only into memory on the default microphone. It runs actual device callbacks, provenance checks, resampling, VoiceCapture and Opus with a synthetic energy decision to isolate capture freshness from speech classification. It sends no live microphone audio to a server and writes no PCM recording.

| Measurement | Original rule | Fixed rule |
|---|---:|---:|
| Processed20ms frames across two open periods |59 /70|59 /70|
| Fully tagged public frames |0 /0|41 /67|
| Opus packets |0 /0|41 /67|

Raw and processed microphone peaks were nonzero. No device errors occurred. Raw RED/GREEN logs are preserved locally under target/desktop-1.2.0/audio-regression-{red,green}.log in the original checkout. The deterministic test failed under the original rule and passes with the fix. The new rule accepts equal callback instants only after a strictly advancing post-generation callback establishes a cutoff. Capture positions must strictly advance, callbacks cannot regress, stale backlog and wrong generations stay private. Private-marker vs zero exact Opus regressions cover44.1/48kHz input chunks assembled from repeated-timestamp10ms callbacks.

This verifies capture-to-Opus eligibility, not human speech timbre, acoustic echo cancellation on two PCs, or delivery through the Tauri UI.

## Microphone preview and display
- Test microphone suppresses effective live Hear myself playback without changing the saved setting; the latest user choice takes effect when testing ends.
- Local test uses actual VAD, threshold and PTT decisions with its own onset buffer. The public network gate remains closed; private preview PCM does not seed the public encoder/preroll.
- Level and voice activation are displayed separately; device/session/processing loss clears stale meter, activation and test state.
- Hidden voice settings no longer enumerate audio devices at application startup. Opening/reopening refreshes the device list.
- Failed voice joining produces visible feedback rather than only a console entry.

## UI, languages and motion
One actual SolidJS interface, Claude Dark default and owner-supplied Finch Custom alternative. Russian default, English selectable, both persisted on this computer. Existing chat/voice/sharing/admin/trust functions retained. Appearance migration repairs missing/invalid values and preserves unrelated preferences. Explicit reactive translation and interpolation; server/user content remains unchanged.

Browser visual checks use real application components with isolated fixture data, not network/hardware simulation evidence. Screens include auth, chat, voice, audio settings, appearance, admin and sharing. Desktop baseline1280x720 and minimum900x600; inspect both themes, RU/EN live switching, readable participant names, scrollable settings/tables, keyboard focus and reduced motion. Screenshots are local under original-checkout target/desktop-1.2.0/screens. Final matrix completion is recorded below.

Motion has short functional enter/exit and control feedback; frequent voice controls do not wait for it. OS/application reduced motion disables travel and transitions. Closing dialogs become inert immediately, restore focus and cancel pending removal when reopened. Cut-corner background layers do not clip popovers or focus rings.

## Automated evidence before final release
| Check | Result |
|---|---|
| Frontend full suite,18Oct01:49 |289passed /33files,0failed|
| Frontend typecheck +production Vite build |PASS, no missing-font warning after public path correction|
| Full workspace Rust before final version/tray/capture changes |412passed /64suite summaries,0failed,2ignored; baseline evidence only|
| Updated app tests incl capture fix |41passed (38unit +3security integration);2hardware ignored in routine suite|
| Explicit real-device repeated-clock pipeline |PASS, before/after measurements above|
| Updated app all-target Clippy -Dwarnings |PASS|
| Final main CI full workspace on release source |PENDING|
| Packaged client/native DLL closure |PENDING|
| Both server/portable ZIP launchers and API/backup/restore |PENDING|
| Fresh install +v1.1.0 upgrade +uninstall (clean runner) |PENDING|
| Updater signature, tamper rejection, manifest URLs/version |PENDING|
| Actual published assets SHA256/signature/download |PENDING|
| Real Tauri window/event delivery |PENDING|

## Limits and scope decisions
No new custom EQ/compressor, NS stacking, arbitrary gain increase, advanced processing controls, new server roles, audit-chain claims, hardware decoder switches or unsupported sharing settings. Those demonstration controls were not product requirements: the owner prioritised a simple working production release. Existing WebRTC/RNNoise/Opus DSP is preserved.

A physical two-PC listening test, different microphone/headset models, NAT conditions and the old client updater confirmation UI remain manual checks; do not infer them from synthetic or headless tests. The installer upgrade script checks isolated installation, binary/native runtime equality and a preserved profile marker; preference migration is separately covered by frontend tests.
