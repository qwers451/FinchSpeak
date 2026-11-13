import { createEffect, type JSX } from 'solid-js';
import { rollIn } from './motion';

type Props = { value: string | number; class?: string };

/**
 * A value that rolls when it changes: up when it grows, down when it shrinks (text does not count as growing).
 * The animation runs on the change only, so a re-render with the same value does nothing.
 */
export function Rolling(props: Props): JSX.Element {
  let element: HTMLSpanElement | undefined;
  // Kept by hand: Solid's on(..., { defer: true }) does not record the previous input on its first run.
  let last = props.value;
  createEffect(() => {
    const value = props.value;
    if (value === last) return;
    const previous = last;
    last = value;
    if (!element) return;
    const shrank = typeof value === 'number' && typeof previous === 'number' && value < previous;
    rollIn(element, shrank ? -1 : 1);
  });
  return <span ref={element} class={`rolling${props.class ? ` ${props.class}` : ''}`}>{props.value}</span>;
}
