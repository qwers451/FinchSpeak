import { createSignal } from 'solid-js';
import { render } from 'solid-js/web';
import { afterEach, expect, it, vi } from 'vitest';
import { Modal } from './Modal';
let dispose: (() => void) | undefined;
afterEach(() => { dispose?.(); document.body.innerHTML = ''; document.documentElement.dataset.reduceMotion = 'false'; vi.useRealTimers(); });
function mount() {
  const host = document.createElement('div'); document.body.append(host);
  const [open, setOpen] = createSignal(true);
  dispose = render(() => <Modal open={open()} onClose={() => setOpen(false)} title="Example"><button>Inside</button></Modal>, host);
  return { host, setOpen };
}
it('removes closing dialogs from interaction immediately and unmounts after the short exit', () => {
  vi.useFakeTimers(); const { host, setOpen } = mount(); setOpen(false);
  expect(host.querySelector('.overlay[data-exiting="true"]')).not.toBeNull();
  expect(host.querySelector('[role="dialog"]')).toBeNull();
  expect(host.querySelector('.overlay')?.hasAttribute('inert')).toBe(true);
  vi.advanceTimersByTime(160); expect(host.querySelector('.overlay')).toBeNull();
});
it('reopening during an exit cancels stale removal', () => {
  vi.useFakeTimers(); const { host, setOpen } = mount(); setOpen(false); setOpen(true); vi.advanceTimersByTime(300);
  expect(host.querySelector('[role="dialog"]')).not.toBeNull();
  expect(host.querySelector('.overlay')?.hasAttribute('inert')).toBe(false);
});
it('reduced motion closes immediately', () => {
  document.documentElement.dataset.reduceMotion = 'true'; const { host, setOpen } = mount(); setOpen(false);
  expect(host.querySelector('.overlay')).toBeNull();
});
it('Escape dismisses immediately for keyboard users', () => {
  const { host } = mount(); host.querySelector('.overlay')!.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true }));
  expect(host.querySelector('.overlay')).toBeNull();
});
