"""Throwaway research probe; no microphone access, no PCM output files.

Calls the shipped AEC DLL with generated fixtures. Models EchoCanceller's
sticky has_reference and 20 ms Rust frames (two 10 ms native blocks).
Omits RNNoise, AGC, device clocks and acoustic paths: not an end-to-end test.
"""
import argparse
import ctypes as C
import hashlib
import json
import math
from pathlib import Path
import struct
import wave

ROOT = Path(__file__).resolve().parent
N = 960
RATE = 48000


def power(x):
    return sum(v*v for v in x) / max(1, len(x))


def db(ratio):
    return 10 * math.log10(max(ratio, 1e-15))


def fixture(path):
    with wave.open(str(path), 'rb') as w:
        assert (w.getnchannels(), w.getsampwidth(), w.getframerate()) == (1, 2, 16000)
        raw = w.readframes(w.getnframes())
    x = [v / 32768 for v in struct.unpack('<' + 'h'*(len(raw)//2), raw)]
    # Independent linear 3x upsampling, identical for every experiment.
    y = [x[i] + (x[min(i+1, len(x)-1)]-x[i])*j/3
         for i in range(len(x)) for j in range(3)]
    gain = math.sqrt(0.001 / max(power(y), 1e-15))
    return [v*gain for v in y]


class Aec:
    def __init__(self, lib, delay):
        self.lib = lib
        self.handle = C.c_void_p()
        self.check(lib.finch_webrtc_create(0, C.byref(self.handle)))
        self.check(lib.finch_webrtc_set_delay(self.handle, delay))
        self.has_reference = False

    @staticmethod
    def check(code):
        if code != 0:
            raise RuntimeError(f'Native AEC error {code}')

    def step(self, capture, render, force_native=False):
        for start in (0, 480):
            source = (C.c_float*480)(*render[start:start+480])
            self.check(self.lib.finch_webrtc_render(self.handle, source, 480))
        self.has_reference |= any(abs(v) > 1e-7 for v in render)
        out = []
        for start in (0, 480):
            source = (C.c_float*480)(*capture[start:start+480])
            target = (C.c_float*480)()
            self.check(self.lib.finch_webrtc_capture(self.handle, source, target, 480))
            out.extend(target)
        assert all(math.isfinite(v) for v in out)
        return out if self.has_reference or force_native else capture.copy()

    def close(self):
        self.lib.finch_webrtc_destroy(self.handle)


def run(lib, source, other, mode, lag_frames, delay, seconds):
    aec = Aec(lib, delay)
    zero = [0.0]*N
    outputs = []
    rows = []
    try:
        for k in range(seconds*50):
            inp = [source[(k*N+i) % len(source)] for i in range(N)]
            if mode == 'closed_loop' and k >= lag_frames:
                ref = outputs[k-lag_frames]
            elif mode == 'delayed_input' and k >= lag_frames:
                ref = [source[((k-lag_frames)*N+i) % len(source)] for i in range(N)]
            elif mode == 'unrelated':
                ref = [other[(k*N+i) % len(other)] for i in range(N)]
            elif mode == 'reference_then_silence' and k < 50:
                ref = [other[(k*N+i) % len(other)] for i in range(N)]
            else:
                ref = zero
            out = aec.step(inp, ref, force_native=(mode == 'silence_native'))
            outputs.append(out)
            rows.append({'ms': k*20, 'input_dbfs': db(power(inp)),
                         'output_dbfs': db(power(out)), 'gain_db': db(power(out)/max(power(inp), 1e-15))})
    finally:
        aec.close()
    # Use 200 ms windows: native latency can bias comparisons of 20 ms frames.
    windows = []
    for k in range(100, len(rows)-9, 10):
        pi = sum(10**(r['input_dbfs']/10) for r in rows[k:k+10])
        po = sum(10**(r['output_dbfs']/10) for r in rows[k:k+10])
        if db(pi/10) > -45:
            windows.append(db(po/pi))
    ordered = sorted(windows)
    assert ordered
    return {'mode': mode, 'monitor_delay_ms': lag_frames*20,
            'aec_delay_ms': delay, 'active_windows': len(windows),
            'gain_db_p10': ordered[len(ordered)//10],
            'gain_db_median': ordered[len(ordered)//2],
            'gain_db_p90': ordered[min(len(ordered)-1, len(ordered)*9//10)],
            'windows_below_minus12db': sum(v < -12 for v in windows),
            'windows_below_minus24db': sum(v < -24 for v in windows),
            'timeline_levels_only': rows}


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument('--dll', required=True)
    ap.add_argument('--seconds', type=int, default=16)
    ap.add_argument('--output', default='probe-results.json')
    ap.add_argument('--controls', action='store_true')
    args = ap.parse_args()
    lib = C.CDLL(args.dll)
    ptr = C.POINTER(C.c_float)
    for name, types in {
        'create': [C.c_int, C.POINTER(C.c_void_p)],
        'capture': [C.c_void_p, ptr, ptr, C.c_uint32],
        'render': [C.c_void_p, ptr, C.c_uint32],
        'set_delay': [C.c_void_p, C.c_uint32],
        'destroy': [C.c_void_p],
    }.items():
        f = getattr(lib, 'finch_webrtc_'+name)
        f.argtypes = types
        f.restype = None if name == 'destroy' else C.c_int
    repo = next(p for p in ROOT.parents if (p / 'Cargo.toml').is_file())
    fixtures = repo / 'crates/finch-testbed/fixtures'
    speech = fixture(fixtures/'speech_1.wav')
    other = fixture(fixtures/'speech_2.wav')
    synthetic = [0.06*(0.6+0.4*math.sin(2*math.pi*3*i/RATE)) *
                 (math.sin(2*math.pi*173*i/RATE)+0.4*math.sin(2*math.pi*346*i/RATE))
                 for i in range(RATE*3)]
    results = []
    sources = [('synthetic_harmonics', synthetic), ('synthesized_speech', speech)]
    if args.controls:
        sources = [('synthesized_speech', speech), ('synthesized_speech_3', fixture(fixtures/'speech_3.wav'))]
    for name, signal in sources:
        cases = [('silence', 3, 10), ('unrelated', 3, 10), ('reference_then_silence', 3, 10)]
        cases += [('closed_loop', lag, delay) for lag in (1, 3, 6, 10) for delay in (0, 10, 60)]
        cases += [('delayed_input', 3, 10)]
        if args.controls:
            cases = [('silence', 3, 10), ('silence_native', 3, 10), ('unrelated', 3, 10),
                     ('closed_loop', 3, 10), ('closed_loop', 6, 60)]
        for mode, lag, delay in cases:
            result = run(lib, signal, other, mode, lag, delay, args.seconds)
            result['source'] = name
            results.append(result)
            print(json.dumps({k:v for k,v in result.items() if k != 'timeline_levels_only'}), flush=True)
    channel_cases = {}
    for name, right in [('same', synthetic), ('silent', [0]*len(synthetic)),
                        ('opposite', [-v for v in synthetic])]:
        mono = [(a+b)/2 for a,b in zip(synthetic, right)]
        channel_cases[name] = {'gain_db': db(power(mono)/power(synthetic)), 'exact_zero': all(v == 0 for v in mono)}
    report = {'purpose': 'throwaway isolated AEC probe, not real-device reproduction',
              'dll_sha256': hashlib.sha256(Path(args.dll).read_bytes()).hexdigest(),
              'seconds_per_case': args.seconds, 'cases': results, 'channel_average': channel_cases}
    (ROOT/args.output).write_text(json.dumps(report, indent=2), encoding='utf-8')
    print('Channel average:', channel_cases)


if __name__ == '__main__':
    main()
