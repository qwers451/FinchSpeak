# Native WebRTC audio

FinchSpeak builds the original C++ WebRTC APM distributed in the pinned
`webrtc-audio-processing-sys 2.1.0` source archive. Its PulseAudio standalone
upstream is release2.1 (WebRTC M131). No AEC/AGC DSP sources are modified.
`native/webrtc/dependencies.lock.json` pins the source archive, upstream and
wrapper commits, Abseil source/build files, Meson/Ninja wheels and their SHA256.

## Why the native route is small

The ready-made Rust wrapper assumes Unix archive/tool names. Rather than fork
its broad C++/Rust binding layer or build unrelated WebRTC video/network code,
we use the standalone upstream Meson Windows build and a narrow C ABI. This
is a build-system adaptation from the plan's GN fallback. The original library
retains ownership of all DSP state. RNNoise remains the existing nnnoiseless
library/model; no duplicate NS runs inside APM.

Use x64 MSVC cl.exe, not clang-cl: this upstream SIMD source accesses MSVC vector
fields which clang-cl does not implement. C++20 is selected consistently for
APM/Abseil/bridge, because the shipped AGC2 code uses designated initializers.
The exact host compiler/SDK identity is recorded with each staged build.

## Build

On Windows enter an x64 MSVC developer environment, install Python3.13.3, then:

```powershell
./scripts/prepare-webrtc.ps1 -Configuration Release
./scripts/prepare-webrtc.ps1 -Configuration Debug -Offline
```

`FINCH_NATIVE_PYTHON` can point to Python explicitly; `FINCH_NATIVE_COMPILER` can
point to x64 cl.exe. On this development machine the existing MozillaBuild
MSVC/SDK installation is also detected. No global PATH or tool installation
is modified. Build-only Meson/Ninja tools stay under target/native-cache.

Acquisition verifies every pinned SHA256. Once cache is complete, -Offline
forbids dependency downloads, including Meson wraps. Missing or modified files
fail the build. Native outputs/provenance are staged in target/native/Release
or Debug and FINCH_WEBRTC_ROOT is set for subsequent Cargo commands in that
PowerShell process. Source/build outputs are ignored by Git.

Native objects/probe and eventual C ABI DLL use /MT; no C++ object or allocation
is owned/freed by Rust across the DLL boundary. The DLL must ship beside the
client executable. The client should run with only the normal Windows system
PATH; MSVC/Python/Meson/Ninja are never runtime dependencies.

## Verification

The probe creates100 AEC-only/AGC-only pairs and processes100 blocks per pair.
Native audio workflow runs Debug/Release, offline warm-cache and a stripped
PATH subprocess. Passing the probe establishes build/initialization/ownership,
not subjective audio quality; the full production capture and real-time media
checks remain required before publishing.

Build gate verified: GitHub Native audio run39013530184 succeeded (clean Windows Release/Debug, offline warm-cache, system PATH). Local Debug/Release and a source/cache path containing spaces and Cyrillic also passed. This is a build gate, not release-quality acceptance.
