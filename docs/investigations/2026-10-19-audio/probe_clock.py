"""Throwaway model of d6275d2 CaptureClock. Synthetic timestamps only.

Demonstrates consequences of invalid timestamp patterns, not that a device
actually emits them. Mirrors the branch conditions in capture_privacy.rs.
"""
import json
from pathlib import Path


class Clock:
    def __init__(self):
        self.generation = 0
        self.first = self.cutoff = self.previous = None

    def tag(self, generation, opened, capture, callback):
        if self.generation != generation or not opened:
            self.generation = generation
            self.first = self.cutoff = self.previous = None
        if not opened or generation == 0 or capture > callback:
            return 0
        if self.previous:
            lc, lb = self.previous
            if capture <= lc or callback < lb:
                return 0
        self.previous = capture, callback
        if self.first is None:
            self.first = callback
            return 0
        if self.cutoff is None:
            if callback <= self.first:
                return 0
            self.cutoff = callback
            return 0
        return generation if capture >= self.cutoff else 0


def source_checks():
    c = Clock()
    data = [(1,90,0),(2,90,0),(3,100,0),(95,110,0),(100,120,32),
            (101,120,32),(100,121,0),(102,119,0),(103,122,32)]
    for capture, callback, expected in data:
        assert c.tag(32, True, capture, callback) == expected


def simulate(pattern):
    c = Clock()
    tags = []
    gate = False
    resets = rejected = 0
    for i in range(1000):
        capture = i*10
        callback = capture+10
        if pattern == 'capture_repeats' and i > 20 and i % 20 == 0:
            capture -= 10
        if pattern == 'callback_repeats' and i % 2 == 1:
            callback -= 10
        tags.append(c.tag(32, True, capture, callback))
        if len(tags) == 2:  # two 480-sample callbacks per 960-sample frame
            fresh = all(t == 32 for t in tags)
            if i > 20:
                rejected += not fresh
                resets += fresh and not gate
            gate = fresh
            tags.clear()
    return {'pattern': pattern, 'duration_ms': 10000,
            'private_frames_after_warmup': rejected,
            'dsp_reopens_after_warmup': resets}


if __name__ == '__main__':
    source_checks()
    results = [simulate(s) for s in ('normal', 'callback_repeats', 'capture_repeats')]
    assert results[0]['dsp_reopens_after_warmup'] == 0
    assert results[1]['dsp_reopens_after_warmup'] == 0
    assert results[2]['dsp_reopens_after_warmup'] > 0
    Path(__file__).with_name('probe-clock-results.json').write_text(
        json.dumps({'scope': 'synthetic model, no actual device timestamps', 'cases': results}, indent=2), encoding='utf-8')
    print(json.dumps(results, indent=2))
