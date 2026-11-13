import { createEffect } from 'solid-js';
import { popIn } from './motion';

type Props = { count: number; mention?: boolean; title?: string };

/** An unread counter that pops when the number grows (not when it shrinks or stays the same). */
export function Badge(props: Props) {
  let element: HTMLSpanElement | undefined;
  let last = props.count;
  createEffect(() => {
    const count = props.count;
    if (count > last && element) popIn(element);
    last = count;
  });
  return (
    <span ref={element} class={`badge${props.mention ? ' mention' : ''}`} title={props.title}>
      {props.count > 99 ? '99+' : props.count}
    </span>
  );
}
