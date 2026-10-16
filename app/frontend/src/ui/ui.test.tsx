import { createSignal } from 'solid-js';
import { render } from 'solid-js/web';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { Avatar, Button, ConfirmDialog, Icon, Modal, Slider, ToastHost, Toggle, clearToasts, pushToast, toasts, MAX_TOASTS } from './index';
import { avatarColor, initialOf } from './avatarColor';

let dispose: (() => void) | undefined;
function mount(view: () => any): HTMLElement {
  const host = document.createElement('div');
  document.body.appendChild(host);
  dispose = render(view, host);
  return host;
}

afterEach(() => {
  dispose?.();
  dispose = undefined;
  document.body.innerHTML = '';
  clearToasts();
  vi.useRealTimers();
});

describe('avatar helpers', () => {
  it('picks a stable colour in 1..8', () => {
    expect(avatarColor('alice')).toBe(avatarColor('alice'));
    for (const id of ['a', 'bob', '42', 'ünï']) {
      const c = avatarColor(id);
      expect(c).toBeGreaterThanOrEqual(1);
      expect(c).toBeLessThanOrEqual(8);
    }
  });
  it('takes the first letter in upper case', () => {
    expect(initialOf('alice')).toBe('A');
    expect(initialOf('  ')).toBe('?');
  });
});

describe('Icon', () => {
  it('references the sprite and is hidden from screen readers', () => {
    const host = mount(() => <Icon name="x" />);
    expect(host.querySelector('use')?.getAttribute('href')).toBe('#i-x');
    expect(host.querySelector('svg')?.getAttribute('aria-hidden')).toBe('true');
  });
});

describe('Avatar', () => {
  it('shows the initial and the speaking ring', () => {
    const host = mount(() => <Avatar name="carol" id="c" speaking />);
    const el = host.querySelector('.av');
    expect(el?.textContent).toContain('C');
    expect(el?.classList.contains('speaking')).toBe(true);
  });
});

describe('Button / Toggle / Slider', () => {
  it('calls onClick but not while loading', () => {
    const onClick = vi.fn();
    const [loading, setLoading] = createSignal(false);
    const host = mount(() => <Button loading={loading()} onClick={onClick}>Go</Button>);
    const btn = host.querySelector('button')!;
    btn.click();
    expect(onClick).toHaveBeenCalledTimes(1);
    setLoading(true);
    btn.click();
    expect(onClick).toHaveBeenCalledTimes(1);
  });

  it('Toggle is a switch that reports the new state', () => {
    const onChange = vi.fn();
    const host = mount(() => <Toggle checked={false} label="Mute" onChange={onChange} />);
    const sw = host.querySelector('[role="switch"]') as HTMLElement;
    expect(sw.getAttribute('aria-checked')).toBe('false');
    sw.click();
    expect(onChange).toHaveBeenCalledWith(true);
  });

  it('Slider sets the fill from the value', () => {
    const host = mount(() => <Slider value={50} min={0} max={200} label="Volume" onChange={() => {}} />);
    const input = host.querySelector('input')!;
    expect(input.style.getPropertyValue('--fill')).toBe('25%');
  });
});

describe('Modal', () => {
  it('closes on Esc and gives the dialog an accessible name', () => {
    const onClose = vi.fn();
    const host = mount(() => (
      <Modal open onClose={onClose} title="Profile">
        <button>OK</button>
      </Modal>
    ));
    const dialog = host.querySelector('[role="dialog"]')!;
    expect(document.getElementById(dialog.getAttribute('aria-labelledby')!)?.textContent).toBe('Profile');
    host.querySelector('.overlay')!.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true }));
    expect(onClose).toHaveBeenCalled();
  });

  it('does not close on Esc when not dismissable', () => {
    const onClose = vi.fn();
    const host = mount(() => (
      <Modal open onClose={onClose} title="Trust" dismissable={false}>
        <button>OK</button>
      </Modal>
    ));
    host.querySelector('.overlay')!.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true }));
    expect(onClose).not.toHaveBeenCalled();
  });

  it('ConfirmDialog reports both answers', () => {
    const onConfirm = vi.fn();
    const onCancel = vi.fn();
    const host = mount(() => (
      <ConfirmDialog open title="Delete?" message="Gone." confirmLabel="Delete" danger onConfirm={onConfirm} onCancel={onCancel} />
    ));
    const buttons = Array.from(host.querySelectorAll('button'));
    buttons.find((b) => b.textContent?.includes('Delete'))!.click();
    buttons.find((b) => b.textContent?.includes('Cancel'))!.click();
    expect(onConfirm).toHaveBeenCalledTimes(1);
    expect(onCancel).toHaveBeenCalledTimes(1);
  });
});

describe('toasts', () => {
  it('merges identical toasts and keeps at most MAX_TOASTS', () => {
    pushToast({ title: 'same' });
    pushToast({ title: 'same' });
    expect(toasts()).toHaveLength(1);
    for (let i = 0; i < MAX_TOASTS + 2; i++) pushToast({ title: `t${i}` });
    expect(toasts()).toHaveLength(MAX_TOASTS);
    expect(toasts().at(-1)?.title).toBe(`t${MAX_TOASTS + 1}`);
  });

  it('closes by itself, but errors stay', () => {
    vi.useFakeTimers();
    pushToast({ kind: 'success', title: 'saved' });
    pushToast({ kind: 'error', title: 'broken' });
    vi.advanceTimersByTime(10_000);
    expect(toasts().map((t) => t.title)).toEqual(['broken']);
  });

  it('ToastHost renders them', () => {
    const host = mount(() => <ToastHost />);
    pushToast({ kind: 'error', title: 'Upload failed' });
    expect(host.textContent).toContain('Upload failed');
    expect(host.querySelector('[role="alert"]')).not.toBeNull();
  });
});
