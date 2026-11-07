import { afterEach, describe, expect, it, vi } from 'vitest';
import { render } from 'solid-js/web';
import { clearToasts, pauseToast, pushToast, resumeToast, toasts } from './toastStore';
import { ToastHost } from './ToastHost';
let dispose: (() => void) | undefined;
afterEach(() => { dispose?.(); dispose = undefined; clearToasts(); document.body.innerHTML = ''; vi.useRealTimers(); });
describe('toast reading time', () => {
  it('resumes the remaining lifetime instead of restarting or expiring during reading', () => {
    vi.useFakeTimers();
    const id = pushToast({ title: 'Saved' });
    vi.advanceTimersByTime(2000); pauseToast(id, 'pointer');
    vi.advanceTimersByTime(20000); expect(toasts()).toHaveLength(1);
    resumeToast(id, 'pointer'); vi.advanceTimersByTime(3999); expect(toasts()).toHaveLength(1);
    vi.advanceTimersByTime(1); expect(toasts()).toHaveLength(0);
  });
  it('waits for both pointer and keyboard focus to leave, ignoring repeated events', () => {
    vi.useFakeTimers(); const id = pushToast({ title: 'Saved' });
    pauseToast(id, 'pointer'); pauseToast(id, 'focus'); pauseToast(id, 'focus');
    resumeToast(id, 'pointer'); resumeToast(id, 'pointer');
    vi.advanceTimersByTime(10000); expect(toasts()).toHaveLength(1);
    resumeToast(id, 'focus'); vi.advanceTimersByTime(6000); expect(toasts()).toHaveLength(0);
  });
  it('does not give sticky errors an expiration timer', () => {
    vi.useFakeTimers(); const id = pushToast({ title: 'Failed', kind: 'error' });
    pauseToast(id, 'focus'); resumeToast(id, 'focus');
    vi.advanceTimersByTime(60000); expect(toasts()).toHaveLength(1);
  });
  it('keeps focus paused between controls, and releases it when the host unmounts', () => {
    vi.useFakeTimers();
    const host = document.createElement('div'); document.body.append(host);
    dispose = render(() => <ToastHost />, host);
    pushToast({ title: 'Retry', action: { label: 'Retry', run: () => {} } });
    const buttons = host.querySelectorAll('button');
    buttons[0].dispatchEvent(new FocusEvent('focusin', { bubbles: true }));
    buttons[0].dispatchEvent(new FocusEvent('focusout', { bubbles: true, relatedTarget: buttons[1] }));
    vi.advanceTimersByTime(10000); expect(toasts()).toHaveLength(1);
    dispose(); dispose = undefined;
    vi.advanceTimersByTime(6000); expect(toasts()).toHaveLength(0);
  });
});
