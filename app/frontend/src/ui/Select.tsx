import { createEffect, createSignal, createUniqueId, For, onCleanup, Show } from 'solid-js';
import { Portal } from 'solid-js/web';
import { Icon } from './Icon';
import { placePopover } from './popover';

export type SelectProps<T extends string> = {
  value: T;
  options: readonly { value: T; label: string; disabled?: boolean }[];
  onChange: (value: T) => void;
  label: string;
};

export function Select<T extends string>(props: SelectProps<T>) {
  const id = createUniqueId();
  let trigger!: HTMLButtonElement;
  let list: HTMLDivElement | undefined;
  const [open, setOpen] = createSignal(false);
  const [active, setActive] = createSignal(-1);
  const enabled = () => props.options.map((option, index) => option.disabled ? -1 : index).filter((index) => index >= 0);
  const close = (restore = true) => { setOpen(false); if (restore) trigger.focus(); };
  const expand = (last = false) => {
    const selected = props.options.findIndex((option) => option.value === props.value && !option.disabled);
    const choices = enabled();
    setActive(selected >= 0 ? selected : choices[last ? choices.length - 1 : 0] ?? -1);
    setOpen(true);
  };
  const choose = (index: number) => {
    const option = props.options[index];
    if (!option || option.disabled) return;
    props.onChange(option.value);
    close();
  };
  const move = (key: string) => {
    const choices = enabled();
    const index = choices.indexOf(active());
    setActive(key === 'Home' ? choices[0] ?? -1 : key === 'End' ? choices[choices.length - 1] ?? -1 :
      choices[(index + (key === 'ArrowDown' ? 1 : -1) + choices.length) % choices.length] ?? -1);
    list?.querySelector<HTMLElement>(`[id="${id}-${active()}"]`)?.scrollIntoView?.({ block: 'nearest' });
  };
  createEffect(() => {
    if (!open()) return;
    queueMicrotask(() => {
      if (!list || !open()) return;
      list.style.setProperty('--popover-min-width', `${Math.min(trigger.getBoundingClientRect().width, window.innerWidth - 16)}px`);
      placePopover(list, trigger);
      list.focus();
    });
    const outside = (event: Event) => {
      if (!list?.contains(event.target as Node) && !trigger.contains(event.target as Node)) close(false);
    };
    const dismiss = () => close();
    document.addEventListener('mousedown', outside, true);
    window.addEventListener('scroll', outside, true);
    window.addEventListener('resize', dismiss);
    window.addEventListener('blur', dismiss);
    onCleanup(() => {
      document.removeEventListener('mousedown', outside, true);
      window.removeEventListener('scroll', outside, true);
      window.removeEventListener('resize', dismiss);
      window.removeEventListener('blur', dismiss);
    });
  });
  return <>
    <button ref={trigger} type="button" class="select" role="combobox" aria-label={props.label} aria-haspopup="listbox"
      aria-expanded={open()} aria-controls={open() ? id : undefined}
      onClick={() => open() ? close() : expand()}
      onKeyDown={(event) => {
        if (event.key === 'ArrowDown' || event.key === 'ArrowUp') { event.preventDefault(); expand(event.key === 'ArrowUp'); }
      }}>
      <span class="trunc">{props.options.find((option) => option.value === props.value)?.label ?? props.value}</span>
      <Icon name="chev-d" size="sm" />
    </button>
    <Show when={open()}><Portal><div class="popover-host">
      <div ref={list} id={id} class="select-listbox floating-panel" role="listbox" tabindex="-1" aria-label={props.label}
        aria-activedescendant={active() >= 0 ? `${id}-${active()}` : undefined}
        onKeyDown={(event) => {
          if (['ArrowDown', 'ArrowUp', 'Home', 'End'].includes(event.key)) { event.preventDefault(); move(event.key); }
          else if (event.key === 'Enter' || event.key === ' ') { event.preventDefault(); choose(active()); }
          else if (event.key === 'Escape') { event.preventDefault(); event.stopPropagation(); close(); }
          else if (event.key === 'Tab') close();
        }}>
        <For each={props.options}>{(option, index) =>
          <div id={`${id}-${index()}`} class="select-option" role="option" aria-selected={option.value === props.value}
            aria-disabled={option.disabled} classList={{ active: active() === index() }}
            onPointerMove={() => { if (!option.disabled) setActive(index()); }} onClick={() => choose(index())}>
            <span class="grow">{option.label}</span><Show when={option.value === props.value}><span aria-hidden="true">✓</span></Show>
          </div>
        }</For>
      </div>
    </div></Portal></Show>
  </>;
}
