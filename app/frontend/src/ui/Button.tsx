import { Show, splitProps, type JSX } from 'solid-js';
import { Icon, type IconName } from './Icon';

type ButtonProps = Omit<JSX.ButtonHTMLAttributes<HTMLButtonElement>, 'class'> & {
  variant?: 'primary' | 'secondary' | 'ghost' | 'violet' | 'danger' | 'danger-ghost';
  size?: 'sm' | 'md' | 'lg';
  /** Square icon button; then `label` is required for screen readers. */
  iconOnly?: boolean;
  icon?: IconName;
  loading?: boolean;
  block?: boolean;
  /** Accessible name for icon-only buttons. */
  label?: string;
  class?: string;
};

export function Button(props: ButtonProps) {
  const [own, rest] = splitProps(props, ['variant', 'size', 'iconOnly', 'icon', 'loading', 'block', 'label', 'class', 'children', 'disabled']);
  const classes = () =>
    ['btn', own.variant && own.variant !== 'ghost' ? own.variant : own.variant, own.size === 'sm' ? 'sm' : own.size === 'lg' ? 'lg' : '', own.iconOnly ? 'icon' : '', own.block ? 'block' : '', own.loading ? 'is-loading' : '', own.class]
      .filter(Boolean)
      .join(' ');
  return (
    <button
      type="button"
      {...rest}
      class={classes()}
      disabled={own.disabled || own.loading}
      aria-label={own.label ?? (rest as { 'aria-label'?: string })['aria-label']}
      aria-busy={own.loading ? 'true' : undefined}
    >
      <Show when={own.loading}><span class="spin" /></Show>
      <Show when={own.icon && !own.loading}><Icon name={own.icon!} size={own.size === 'sm' || own.iconOnly ? 'sm' : undefined} /></Show>
      {own.children}
    </button>
  );
}
