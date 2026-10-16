import { createSignal, createUniqueId, Show, type JSX } from 'solid-js';
import { Icon, type IconName } from './Icon';

type FieldProps = {
  label: string;
  value: string;
  onInput: (value: string) => void;
  type?: 'text' | 'password';
  icon?: IconName;
  placeholder?: string;
  error?: string | null;
  help?: string;
  autocomplete?: string;
  autofocus?: boolean;
  disabled?: boolean;
  required?: boolean;
  /** Extra content on the right inside the box (e.g. a tag). */
  trailing?: JSX.Element;
};

/** Label + input box + error/help line, as in the design's `.field`. Passwords get a show/hide eye. */
export function Field(props: FieldProps) {
  const id = createUniqueId();
  const [shown, setShown] = createSignal(false);
  const isPassword = () => props.type === 'password';
  return (
    <div class="field">
      <label class="field-label" for={id}>{props.label}</label>
      <div class={`input${props.error ? ' is-error' : ''}${props.disabled ? ' is-disabled' : ''}`}>
        <Show when={props.icon}><Icon name={props.icon!} size="sm" class="t3" /></Show>
        <input
          id={id}
          type={isPassword() && !shown() ? 'password' : 'text'}
          value={props.value}
          placeholder={props.placeholder}
          autocomplete={props.autocomplete ?? 'off'}
          autofocus={props.autofocus}
          disabled={props.disabled}
          required={props.required}
          spellcheck={false}
          aria-invalid={props.error ? 'true' : undefined}
          aria-describedby={props.error ? `${id}-e` : props.help ? `${id}-h` : undefined}
          onInput={(event) => props.onInput(event.currentTarget.value)}
        />
        {props.trailing}
        <Show when={isPassword()}>
          <button type="button" class="icon-close" aria-label={shown() ? 'Hide password' : 'Show password'} onClick={() => setShown(!shown())}>
            <Icon name="eye" size="sm" />
          </button>
        </Show>
      </div>
      <Show when={props.error}>
        <span id={`${id}-e`} class="err"><Icon name="warn" size="sm" />{props.error}</span>
      </Show>
      <Show when={!props.error && props.help}><span id={`${id}-h`} class="help">{props.help}</span></Show>
    </div>
  );
}
