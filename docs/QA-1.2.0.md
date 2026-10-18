# FinchSpeak 1.2.0 — QA and release evidence

Status: stable v1.2.0 published at https://github.com/qwers451/FinchSpeak/releases/tag/v1.2.0. Updated 18 October 2026 (Europe/Moscow). Release source: 6d751b04a7f272251ba66c9dc90bd843c4bbff5f. Baseline: main9f500ce / published v1.1.0 source807a893. Product scope and deliberate omissions are in docs/superpowers/specs/2026-10-17-desktop-1.2.0-design.md.

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

The owner's updated kit supplied a new bird. Its mark and illustrations replaced the earlier kit art; Claude Dark now uses the same bird, and Tauri app/installer icons are generated from that mark. The native tray decodes the supplied 64px online, voice and muted PNGs, with a visible red live-sharing marker. Its asset decoding and three distinct states have a focused unit test. Browser checks confirmed the updated Claude authentication artwork, both voice-room skins, Russian admin labels and the repaired Custom mention/search popovers. Source images were read as data; kit scripts were not executed.

A final brand audit found that the README and a few direct component image sources still pointed to older public assets even though theme CSS displayed the new kit artwork in the main views. All direct image references and the favicon now point to the updated kit's canonical mark/mascot; the three obsolete public files were removed. The kit SVG content was compared with the supplied ZIP after newline normalization. The rebuilt frontend contains both canonical assets, and the final frontend suite passed again.

An actual EXE resource check caught a cached old client icon and the default NSIS installer icon. `app/build.rs` now tracks the ICO and Tauri config changes; NSIS explicitly uses the same ICO. After rebuilding, the client and installer embedded 32px icon pixels both exactly matched the supplied bird. `scripts/test-release-icons.ps1` enforces this in release CI.

Motion has short functional enter/exit and control feedback; frequent voice controls do not wait for it. OS/application reduced motion disables travel and transitions. Closing dialogs become inert immediately, restore focus and cancel pending removal when reopened. Cut-corner background layers do not clip popovers or focus rings.

## Automated and published release evidence
| Check | Result |
|---|---|
| Frontend full suite after review fixes |289passed /33files,0failed|
| Frontend typecheck +production Vite build after updated art |PASS, 85 modules, no missing-font warning|
| Full workspace Rust on 1.2.0 feature source |415passed /64suite summaries,0failed,3hardware ignored|
| Final app tests after icon/config change |42passed (39unit +3security integration),0failed,2hardware ignored|
| Explicit real-device repeated-clock pipeline |PASS, before/after measurements above|
| Updated workspace all-target Clippy -Dwarnings |PASS after icon/config change|
| Final main CI full workspace on release source |PASS, run 39122229526 on 6d751b0|
| Tag-triggered Windows release workflow |PASS, run 39122300002 on the same source|
| Packaged client/native DLL closure and client/installer embedded bird icons |PASS on final binaries|
| Both final ZIP launchers and API/backup/restore |PASS on final archives|
| Fresh install +v1.1.0 upgrade +uninstall (isolated profile) |PASS on final NSIS|
| Updater signature, tamper rejection, manifest URLs/version and five local SHA256 sums |PASS on final NSIS/manifest/packages|
| Actual published assets SHA256/signature/download |PASS, all six expected assets present; all five hashes match; signed installer and updater manifest accepted by bundled public key; tampered installer rejected|
| Published ZIP server launcher and API/backup smoke test |PASS on downloaded server archive|
| Published client and installer embedded bird icons |PASS, exact source icon pixel check on downloaded portable EXE and installer|
| Real Tauri window/event delivery |Process stays open under isolated profile; UI/event inspection PENDING (Windows automation helper unavailable)|

An independent read-only source review reported no remaining Critical or Important findings in the audio/privacy path, preference migration, localization, popovers, artwork or updater. Its report is preserved locally under original-checkout target/desktop-1.2.0/final-review.md. The reviewer inspected source only; packaged checks above were run separately.

## Limits and scope decisions
No new custom EQ/compressor, NS stacking, arbitrary gain increase, advanced processing controls, new server roles, audit-chain claims, hardware decoder switches or unsupported sharing settings. Those demonstration controls were not product requirements: the owner prioritised a simple working production release. Existing WebRTC/RNNoise/Opus DSP is preserved.

A physical two-PC listening test, different microphone/headset models, NAT conditions and the old client updater confirmation UI remain manual checks; do not infer them from synthetic or headless tests. The installer upgrade script checks isolated installation, binary/native runtime equality and a preserved profile marker; preference migration is separately covered by frontend tests.
