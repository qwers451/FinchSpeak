import { createSignal } from 'solid-js';
import { render } from 'solid-js/web';
import { afterEach, expect, it, vi } from 'vitest';
import { Select } from './Select';
let dispose: (() => void) | undefined;
const flush = () => new Promise((resolve) => setTimeout(resolve, 0));
const press = (key: string) => document.activeElement?.dispatchEvent(new KeyboardEvent('keydown', { key, bubbles: true, cancelable: true }));
afterEach(() => { dispose?.(); document.body.innerHTML = ''; vi.restoreAllMocks(); });
function mount(value = 'a') {
  const [selected, setSelected] = createSignal(value);
  const root = document.createElement('div'); document.body.append(root);
  const change = vi.fn((next: string) => setSelected(next));
  dispose = render(() => <Select label="Device" value={selected()} options={[{ value: 'a', label: 'Microphone' }, { value: 'b', label: 'Unavailable', disabled: true }, { value: 'c', label: 'Headset' }]} onChange={change} />, root);
  const trigger = root.querySelector<HTMLButtonElement>('[role="combobox"]')!;
  return { trigger, change, setSelected };
}
it('supports keyboard selection, skips disabled options and returns focus', async () => {
  const { trigger, change } = mount();
  trigger.focus(); press('ArrowDown'); await flush();
  const list = document.querySelector('[role="listbox"]')!;
  expect(list.getAttribute('aria-label')).toBe('Device');
  expect(document.activeElement).toBe(list);
  press('ArrowDown'); press('Enter');
  expect(change).toHaveBeenCalledWith('c');
  expect(trigger.textContent).toContain('Headset');
  expect(document.activeElement).toBe(trigger);
  expect(document.querySelector('[role="listbox"]')).toBeNull();
  trigger.click(); await flush(); press('Home'); press(' '); expect(change).toHaveBeenLastCalledWith('a');
});
it('shows a missing value, cancels with Escape and closes on outside click', async () => {
  const { trigger, change } = mount('Unplugged USB');
  expect(trigger.textContent).toContain('Unplugged USB');
  trigger.click(); await flush(); press('End'); press('Escape');
  expect(change).not.toHaveBeenCalled();
  expect(document.activeElement).toBe(trigger);
  trigger.click(); await flush();
  document.body.dispatchEvent(new MouseEvent('mousedown', { bubbles: true }));
  expect(document.querySelector('[role="listbox"]')).toBeNull();
});
it('keeps the list inside the viewport and marks selected/disabled options', async () => {
  const { trigger } = mount();
  vi.spyOn(HTMLElement.prototype, 'getBoundingClientRect').mockImplementation(function (this: HTMLElement) {
    return this === trigger
      ? { left: window.innerWidth - 50, right: window.innerWidth, top: window.innerHeight - 40, bottom: window.innerHeight, width: 50, height: 40, x: 0, y: 0, toJSON() {} }
      : { left: 0, right: 240, top: 0, bottom: 160, width: 240, height: 160, x: 0, y: 0, toJSON() {} };
  });
  trigger.click(); await flush();
  const list = document.querySelector<HTMLElement>('[role="listbox"]')!;
  expect(parseFloat(list.style.getPropertyValue('--popover-x')) + 240).toBeLessThanOrEqual(window.innerWidth);
  expect(parseFloat(list.style.getPropertyValue('--popover-y')) + 160).toBeLessThanOrEqual(window.innerHeight);
  expect(list.querySelector('[aria-selected="true"]')?.textContent).toContain('Microphone');
  const disabled = list.querySelector<HTMLElement>('[aria-disabled="true"]')!;
  disabled.click(); expect(document.querySelector('[role="listbox"]')).toBeTruthy();
});
