import type { JSX } from 'solid-js';

type ToggleProps = { checked: boolean; onChange: (next: boolean) => void; label: string; disabled?: boolean };

/** A switch. `label` is its accessible name. */
export function Toggle(props: ToggleProps): JSX.Element {
  return (
    <button
      type="button"
      role="switch"
      aria-checked={props.checked}
      aria-label={props.label}
      disabled={props.disabled}
      class={`toggle${props.checked ? ' on' : ''}${props.disabled ? ' is-disabled' : ''}`}
      onClick={() => props.onChange(!props.checked)}
    />
  );
}
