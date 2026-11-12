type SliderProps = {
  value: number;
  min: number;
  max: number;
  step?: number;
  label: string;
  onChange: (value: number) => void;
  /** Fires once the value is settled (pointer released, key pressed), not on every move. */
  onCommit?: (value: number) => void;
  /** Cyan instead of yellow (volumes). */
  cool?: boolean;
  disabled?: boolean;
};

/** A native range input, so the keyboard and screen readers work; styled by `.range`. */
export function Slider(props: SliderProps) {
  const percent = () => ((props.value - props.min) / (props.max - props.min || 1)) * 100;
  return (
    <span class="range-control" data-disabled={props.disabled ? 'true' : undefined}>
    <input
      type="range"
      class={`range${props.cool ? ' cool' : ''}`}
      min={props.min}
      max={props.max}
      step={props.step ?? 1}
      value={props.value}
      disabled={props.disabled}
      aria-label={props.label}
      style={{ '--fill': `${percent()}%` }}
      onInput={(event) => props.onChange(parseFloat(event.currentTarget.value))}
      onChange={(event) => props.onCommit?.(parseFloat(event.currentTarget.value))}
    />
    <span class="range-value" aria-hidden="true" style={{ left: `clamp(24px, ${Math.max(0, Math.min(100, percent()))}%, calc(100% - 24px))` }}>{props.value}</span>
    </span>
  );
}
