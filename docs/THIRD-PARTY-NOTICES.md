# Third-party notices
FinchSpeak 1.2.0 uses the original WebRTC Audio Processing Module (standalone2.1, WebRTC M131), BSD-style licensed with its patent grant. Upstream source revision d0569cfa50c1858ee279d77b3fc8870be6902441, published sys wrapper2.1.0 revision c14d7af1760baff83e8210fee336a0cae0faaa7d. Abseil20240722.0 is Apache2.0 licensed. Source URLs and integrity hashes are in native/webrtc/dependencies.lock.json; build instructions in docs/NATIVE-AUDIO.md. The bridge does not modify upstream DSP.

RNNoise is provided by nnnoiseless0.5.2 (BSD3-Clause); its bundled default pretrained model is used unchanged, without third-party/custom weights. Copyrights and license are in licenses/nnnoiseless-COPYING and AUTHORS. Earshot1.2.2 supplies speech detection; Opus supplies voice encoding. These existing Rust dependencies retain their upstream licenses. Cargo.lock pins the full Rust dependency graph.

The licenses directory contains upstream WebRTC notices, third-party notices, PATENTS, Abseil LICENSE and RNNoise notices. Runtime/compiler sources are not required on the recipient PC. No Microsoft Visual C++ redistributable is intentionally required by the native APM DLL (static CRT); release validation inspects the actual payload imports.

Earshot1.2.2: MIT OR Apache-2.0; license texts acquired from the exact published crate source revision c400f15bd232dda30a297eec90235bd09066cab7 (the crate omits root license files). See licenses/earshot-LICENSE-MIT and earshot-LICENSE-APACHE. Opus BSD notice is licenses/opus-COPYING.


## Interface fonts and supplied design assets
Inter and JetBrains Mono font files bundled with the owner-supplied FinchSpeak UI kit are distributed under the SIL Open Font License 1.1. Their notices are included as licenses/OFL-Inter.txt and licenses/OFL-JetBrainsMono.txt and alongside the bundled fonts. The FinchSpeak custom brand, illustrations and UI glyphs come from the owner-supplied finchspeak-ui-kit rev 2.0; the preview scripts and asset generators are not executed or included in the application. Claude Dark uses original FinchSpeak artwork and styling inspired by the requested visual direction.
