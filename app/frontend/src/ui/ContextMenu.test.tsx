import { render } from 'solid-js/web';
import { afterEach, expect, it, vi } from 'vitest';
import { ContextMenuHost, openContextMenu, closeContextMenu } from './ContextMenu';

let dispose: (() => void) | undefined;
const flush = () => new Promise((resolve) => setTimeout(resolve, 0));
const press = (key: string) => document.activeElement?.dispatchEvent(new KeyboardEvent('keydown', { key, bubbles: true, cancelable: true }));
afterEach(() => { closeContextMenu(); dispose?.(); document.body.innerHTML = ''; vi.useRealTimers(); vi.restoreAllMocks(); });
const settle = async () => { for (let i = 0; i < 5; i++) await Promise.resolve(); };
const entry = (label: string) => [...document.querySelectorAll<HTMLElement>('.ctx-entry')].find((e) => e.textContent?.includes(label))!;

function mount() {
  const root = document.createElement('div');
  const trigger = document.createElement('button');
  document.body.append(root, trigger);
  dispose = render(() => <ContextMenuHost />, root);
  trigger.focus();
  return trigger;
}

it('navigates enabled items and nested menus, then returns focus with Escape', async () => {
  const trigger = mount();
  openContextMenu(trigger, [
    { kind: 'header', label: 'Alice' },
    { kind: 'item', label: 'Disabled', disabled: true, onSelect() {} },
    { kind: 'item', label: 'Mute', checked: true, onSelect() {} },
    { kind: 'separator' },
    { kind: 'submenu', label: 'Move', items: [{ kind: 'item', label: 'Room', onSelect() {} }] },
  ]);
  await flush();
  expect(document.activeElement?.textContent).toContain('Mute');
  expect(document.querySelector('[role="menuitemcheckbox"]')?.getAttribute('aria-checked')).toBe('true');
  press('End');
  press('ArrowRight');
  await flush();
  expect(document.activeElement?.textContent).toContain('Room');
  press('ArrowLeft');
  expect(document.activeElement?.textContent).toContain('Move');
  press('Home');
  expect(document.activeElement?.textContent).toContain('Mute');
  press('Escape');
  expect(document.querySelector('[role="menu"]')).toBeNull();
  expect(document.activeElement).toBe(trigger);
});

it('selects once with Space and lets a slider adjust without closing', async () => {
  mount();
  const change = vi.fn();
  const choose = vi.fn();
  openContextMenu({ x: 20, y: 20 }, [
    { kind: 'slider', label: 'Volume', min: 0, max: 2, step: 0.05, value: 1, onChange: change, format: (v) => `${v * 100}%` },
    { kind: 'item', label: 'Choose', onSelect: choose },
  ]);
  await flush();
  const slider = document.querySelector('input')!;
  slider.value = '1.5';
  slider.dispatchEvent(new Event('input', { bubbles: true }));
  expect(change).toHaveBeenCalledWith(1.5);
  expect(document.querySelector('[role="menu"]')).toBeTruthy();
  press('End');
  press(' ');
  expect(choose).toHaveBeenCalledTimes(1);
  expect(document.querySelector('[role="menu"]')).toBeNull();
});

it('flips at viewport edges and dismisses on outside pointer, scroll, resize and blur', async () => {
  const trigger = mount();
  vi.spyOn(HTMLElement.prototype, 'offsetWidth', 'get').mockReturnValue(240);
  vi.spyOn(HTMLElement.prototype, 'offsetHeight', 'get').mockReturnValue(160);
  for (const event of ['mousedown', 'scroll', 'resize', 'blur']) {
    openContextMenu({ x: window.innerWidth - 1, y: window.innerHeight - 1 }, [{ kind: 'item', label: 'Item', onSelect() {} }]);
    await flush();
    const menu = document.querySelector<HTMLElement>('[role="menu"]')!;
    expect(parseFloat(menu.style.getPropertyValue('--popover-x')) + 240).toBeLessThanOrEqual(window.innerWidth);
    expect(parseFloat(menu.style.getPropertyValue('--popover-y')) + 160).toBeLessThanOrEqual(window.innerHeight);
    (event === 'mousedown' ? trigger : window).dispatchEvent(new Event(event, { bubbles: true }));
    expect(document.querySelector('[role="menu"]')).toBeNull();
  }
});

function mountWithScrollers() {
  const root = document.createElement('div');
  root.innerHTML = '<div class="chat"><button class="anchor">Member</button></div><div class="list"></div>';
  document.body.append(root);
  const host = document.createElement('div');
  document.body.append(host);
  dispose = render(() => <ContextMenuHost />, host);
  return { chat: root.querySelector<HTMLElement>('.chat')!, list: root.querySelector<HTMLElement>('.list')!, anchor: root.querySelector<HTMLElement>('.anchor')! };
}

it('ignores scrolling outside its anchor, closes when the anchor scrolls, and stays open on scroll inside itself', async () => {
  const { chat, list, anchor } = mountWithScrollers();
  openContextMenu(anchor, [{ kind: 'item', label: 'Mute', onSelect() {} }]);
  await flush();
  list.dispatchEvent(new Event('scroll'));
  expect(document.querySelector('[role="menu"]')).toBeTruthy();
  document.querySelector('[role="menu"]')!.dispatchEvent(new Event('scroll'));
  expect(document.querySelector('[role="menu"]')).toBeTruthy();
  chat.dispatchEvent(new Event('scroll'));
  expect(document.querySelector('[role="menu"]')).toBeNull();
});

it('keeps a point-anchored menu open when a non-document container scrolls', async () => {
  mountWithScrollers();
  openContextMenu({ x: 20, y: 20 }, [{ kind: 'item', label: 'Mute', onSelect() {} }]);
  await flush();
  document.querySelector('.list')!.dispatchEvent(new Event('scroll'));
  expect(document.querySelector('[role="menu"]')).toBeTruthy();
});

it('keeps a submenu open while the pointer travels from its trigger into the submenu', async () => {
  vi.useFakeTimers();
  mount();
  openContextMenu({ x: 20, y: 20 }, [
    { kind: 'submenu', label: 'Move', items: [{ kind: 'item', label: 'Room', onSelect() {} }] },
    { kind: 'item', label: 'Leave', onSelect() {} },
  ]);
  await settle();
  entry('Move').dispatchEvent(new Event('pointerenter'));
  await settle();
  expect(document.querySelectorAll('[role="menu"]').length).toBe(2);
  entry('Leave').dispatchEvent(new Event('pointerenter'));
  vi.advanceTimersByTime(60);
  document.querySelectorAll('[role="menu"]')[1].dispatchEvent(new Event('pointerenter'));
  vi.advanceTimersByTime(300);
  await settle();
  expect(document.querySelectorAll('[role="menu"]').length).toBe(2);
  expect(document.body.textContent).toContain('Room');
});

it('closes a submenu about 150 ms after the pointer leaves it for a sibling', async () => {
  vi.useFakeTimers();
  mount();
  openContextMenu({ x: 20, y: 20 }, [
    { kind: 'submenu', label: 'Move', items: [{ kind: 'item', label: 'Room', onSelect() {} }] },
    { kind: 'item', label: 'Leave', onSelect() {} },
  ]);
  await settle();
  entry('Move').dispatchEvent(new Event('pointerenter'));
  await settle();
  entry('Leave').dispatchEvent(new Event('pointerenter'));
  vi.advanceTimersByTime(140);
  expect(document.querySelectorAll('[role="menu"]').length).toBe(2);
  vi.advanceTimersByTime(20);
  await settle();
  expect(document.querySelectorAll('[role="menu"]').length).toBe(1);
});

it('runs the action once on click and closes the menu', async () => {
  mount();
  const onSelect = vi.fn();
  openContextMenu({ x: 20, y: 20 }, [{ kind: 'item', label: 'Mute', onSelect }]);
  await flush();
  entry('Mute').dispatchEvent(new MouseEvent('mousedown', { bubbles: true }));
  expect(document.querySelector('[role="menu"]')).toBeTruthy();
  entry('Mute').click();
  expect(onSelect).toHaveBeenCalledTimes(1);
  expect(document.querySelector('[role="menu"]')).toBeNull();
});
