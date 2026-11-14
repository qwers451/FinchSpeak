import { createEffect, createSignal, For, onCleanup, onMount, Show } from 'solid-js';
import { Portal } from 'solid-js/web';
import { Icon, type IconName } from './Icon';
import { Slider } from './Slider';
import { placePopover, scrollMovesAnchor, type PopoverAnchor } from './popover';

/** A submenu stays open this long after the pointer leaves for a sibling, so the pointer can cross to it. */
const SUBMENU_CLOSE_DELAY = 150;

export type MenuItem =
  | { kind: 'item'; label: string; icon?: IconName; danger?: boolean; disabled?: boolean; checked?: boolean; onSelect: () => void }
  | { kind: 'submenu'; label: string; icon?: IconName; items: MenuItem[] }
  | { kind: 'separator' }
  | { kind: 'header'; label: string }
  | { kind: 'slider'; label: string; min: number; max: number; step: number; value: number; onChange: (value: number) => void; format: (value: number) => string; disabled?: boolean };

/** `scrollAnchor` is the element the menu belongs to: the anchor itself, or for a point the element under it when the menu opened. */
type Request = { anchor: PopoverAnchor; scrollAnchor: Element | null; items: MenuItem[]; previous: HTMLElement | null };
const [request, setRequest] = createSignal<Request | null>(null);

export function openContextMenu(anchor: PopoverAnchor, items: MenuItem[]): void {
  const scrollAnchor = anchor instanceof HTMLElement ? anchor : document.elementFromPoint?.(anchor.x, anchor.y) ?? null;
  setRequest({ anchor, scrollAnchor, items, previous: document.activeElement instanceof HTMLElement ? document.activeElement : null });
}

export function closeContextMenu(restoreFocus = true) {
  const previous = request()?.previous;
  setRequest(null);
  if (restoreFocus && previous?.isConnected) previous.focus();
}

function MenuSlider(props: { item: Extract<MenuItem, { kind: 'slider' }> }) {
  return <label class="ctx-slider">
    <span class="row between g8"><span>{props.item.label}</span><output>{props.item.format(props.item.value)}</output></span>
    <Slider cool label={props.item.label} min={props.item.min} max={props.item.max} step={props.item.step} value={props.item.value} disabled={props.item.disabled}
      onChange={(next) => props.item.onChange(next)} />
  </label>;
}

function MenuPanel(props: { items: MenuItem[]; anchor: PopoverAnchor; parent?: HTMLButtonElement; onBack?: () => void; onEnter?: () => void }) {
  let panel!: HTMLDivElement;
  const [submenu, setSubmenu] = createSignal<{ item: Extract<MenuItem, { kind: 'submenu' }>; button: HTMLButtonElement } | null>(null);
  let closeTimer: ReturnType<typeof setTimeout> | undefined;
  const cancelClose = () => { clearTimeout(closeTimer); closeTimer = undefined; };
  /** The pointer left the submenu's trigger for a sibling: close only if it does not reach the submenu in time. */
  const closeSoon = () => {
    cancelClose();
    if (!submenu()) return;
    closeTimer = setTimeout(() => { closeTimer = undefined; setSubmenu(null); }, SUBMENU_CLOSE_DELAY);
  };
  /** Closes the submenu at once (keyboard focus, back) and drops any pending close. */
  const closeNow = () => { cancelClose(); setSubmenu(null); };
  onCleanup(cancelClose);
  const focusable = () => [...panel.querySelectorAll<HTMLElement>('button:not(:disabled), input:not(:disabled)')];
  onMount(() => {
    placePopover(panel, props.anchor, !!props.parent);
    queueMicrotask(() => { if (panel.isConnected) focusable()[0]?.focus(); });
  });
  const onKeyDown = (event: KeyboardEvent) => {
    const entries = focusable();
    const index = entries.indexOf(document.activeElement as HTMLElement);
    if (['ArrowDown', 'ArrowUp', 'Home', 'End'].includes(event.key)) {
      event.preventDefault();
      const next = event.key === 'Home' ? 0 : event.key === 'End' ? entries.length - 1 :
        (index + (event.key === 'ArrowDown' ? 1 : -1) + entries.length) % entries.length;
      entries[next]?.focus();
    } else if (event.key === 'ArrowLeft' && props.parent) {
      event.preventDefault();
      props.onBack?.();
      props.parent.focus();
    } else if (event.key === 'Escape' || event.key === 'Tab') {
      event.stopPropagation();
      if (event.key === 'Escape') event.preventDefault();
      closeContextMenu();
    }
  };

  return <>
    <div ref={panel} class="ctx-menu floating-panel" role="menu" onKeyDown={onKeyDown} onPointerEnter={() => props.onEnter?.()}>
      <For each={props.items}>{(item) => {
        if (item.kind === 'separator') return <div class="ctx-separator" role="separator" />;
        if (item.kind === 'header') return <div class="ctx-header">{item.label}</div>;
        if (item.kind === 'slider') return <MenuSlider item={item} />;
        if (item.kind === 'submenu') {
          const open = (button: HTMLButtonElement) => setSubmenu({ item, button });
          return <button type="button" class="ctx-entry" tabindex="-1" role="menuitem" aria-haspopup="menu" aria-expanded={submenu()?.item === item}
            onPointerEnter={(event) => { cancelClose(); open(event.currentTarget); }} onClick={(event) => open(event.currentTarget)}
            onKeyDown={(event) => { if (event.key === 'ArrowRight') { event.preventDefault(); open(event.currentTarget); } }}>
            <Show when={item.icon}>{(icon) => <Icon name={icon()} size="sm" />}</Show>
            <span class="grow">{item.label}</span><Icon name="chev-r" size="sm" />
          </button>;
        }
        return <button type="button" class="ctx-entry" classList={{ danger: item.danger }} tabindex="-1"
          role={item.checked === undefined ? 'menuitem' : 'menuitemcheckbox'} aria-checked={item.checked} disabled={item.disabled}
          onPointerEnter={closeSoon} onFocus={closeNow}
          onClick={() => { closeContextMenu(); item.onSelect(); }}
          onKeyDown={(event) => {
            if (event.key === 'Enter' || event.key === ' ') { event.preventDefault(); event.currentTarget.click(); }
          }}>
          <Show when={item.icon}>{(icon) => <Icon name={icon()} size="sm" />}</Show>
          <span class="grow">{item.label}</span><Show when={item.checked}><span aria-hidden="true">✓</span></Show>
        </button>;
      }}</For>
    </div>
    <Show when={submenu()} keyed>{(child) => <MenuPanel items={child.item.items} anchor={child.button} parent={child.button} onBack={closeNow} onEnter={cancelClose} />}</Show>
  </>;
}

/** Exactly one host is mounted at the app root. Every menu is portalled out of clipped panels. */
export function ContextMenuHost() {
  let host!: HTMLDivElement;
  createEffect(() => {
    if (!request()) return;
    const outside = (event: Event) => { if (!host?.contains(event.target as Node)) closeContextMenu(false); };
    // Only a scroll of the anchor (or its ancestors, or the document) closes the menu; chat or list scrolling does not.
    const scrolled = (event: Event) => {
      const current = request();
      if (current && !host?.contains(event.target as Node) && scrollMovesAnchor(event.target, current.scrollAnchor)) closeContextMenu(false);
    };
    const dismiss = () => closeContextMenu();
    document.addEventListener('mousedown', outside, true);
    window.addEventListener('scroll', scrolled, true);
    window.addEventListener('resize', dismiss);
    window.addEventListener('blur', dismiss);
    onCleanup(() => {
      document.removeEventListener('mousedown', outside, true);
      window.removeEventListener('scroll', scrolled, true);
      window.removeEventListener('resize', dismiss);
      window.removeEventListener('blur', dismiss);
    });
  });
  onCleanup(() => closeContextMenu(false));
  return <Show when={request()} keyed>{(menu) => <Portal><div ref={host} class="popover-host">
    <MenuPanel items={menu.items} anchor={menu.anchor} />
  </div></Portal>}</Show>;
}
