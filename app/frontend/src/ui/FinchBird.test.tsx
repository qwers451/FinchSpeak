import { createSignal } from 'solid-js';
import { render } from 'solid-js/web';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { FinchBird } from './FinchBird';
import { Rolling } from './Rolling';
import { Badge } from './Badge';

let dispose: (() => void) | undefined;

beforeEach(() => {
  document.body.innerHTML = '';
});

afterEach(() => {
  dispose?.();
  dispose = undefined;
  delete (Element.prototype as Partial<Element> & { animate?: unknown }).animate;
  document.documentElement.removeAttribute('data-reduce-motion');
});

it('embeds the tile bird with the parts the motion CSS moves', () => {
  const root = document.createElement('div');
  document.body.append(root);
  dispose = render(() => <FinchBird size={150} />, root);
  const svg = root.querySelector('svg.finch-bird')!;
  expect(svg).not.toBeNull();
  expect(svg.getAttribute('viewBox')).toBe('0 0 1248 1248');
  expect(svg.getAttribute('width')).toBe('150');
  expect(svg.getAttribute('aria-hidden')).toBe('true');
  expect(svg.querySelectorAll('.bird-eye')).toHaveLength(1);
  expect(svg.querySelectorAll('.bird-waves')).toHaveLength(1);
  expect(svg.querySelectorAll('.bird-waves > .bird-wave')).toHaveLength(2);
});

it('is labelled when it carries meaning, and two birds never share gradient ids', () => {
  const root = document.createElement('div');
  document.body.append(root);
  dispose = render(() => (
    <>
      <FinchBird label="FinchSpeak bird" />
      <FinchBird />
    </>
  ), root);
  const [first, second] = [...root.querySelectorAll('svg.finch-bird')];
  expect(first.getAttribute('role')).toBe('img');
  expect(first.getAttribute('aria-label')).toBe('FinchSpeak bird');
  expect(second.getAttribute('aria-hidden')).toBe('true');
  const firstId = first.querySelector('linearGradient')!.id;
  const secondId = second.querySelector('linearGradient')!.id;
  expect(firstId).not.toBe(secondId);
  const prefix = (id: string) => id.slice(0, id.lastIndexOf('-'));
  expect(first.outerHTML).toContain(`url(#${prefix(firstId)}-`);
  expect(first.outerHTML).not.toContain(prefix(secondId));
});

it('rolls a changed value, and does nothing for an unchanged one or under reduced motion', () => {
  const animate = vi.fn(() => ({ onfinish: null }) as never);
  const [value, setValue] = createSignal<string | number>(3);
  const root = document.createElement('div');
  document.body.append(root);
  dispose = render(() => <Rolling value={value()} />, root);
  (root.querySelector('.rolling') as HTMLElement & { animate?: unknown }).animate = animate;
  expect(root.textContent).toBe('3');
  expect(animate).not.toHaveBeenCalled();

  setValue(3);
  expect(animate).not.toHaveBeenCalled();
  setValue(4);
  expect(animate).toHaveBeenCalledTimes(1);
  expect(root.textContent).toBe('4');

  document.documentElement.dataset.reduceMotion = 'true';
  setValue(5);
  expect(animate).toHaveBeenCalledTimes(1);
});

it('a badge pops only when its count grows', () => {
  const animate = vi.fn(() => ({ onfinish: null }) as never);
  const [count, setCount] = createSignal(1);
  const root = document.createElement('div');
  document.body.append(root);
  dispose = render(() => <Badge count={count()} />, root);
  (root.querySelector('.badge') as HTMLElement & { animate?: unknown }).animate = animate;
  setCount(2);
  expect(animate).toHaveBeenCalledTimes(1);
  setCount(1);
  expect(animate).toHaveBeenCalledTimes(1);
  expect(root.querySelector('.badge')!.textContent).toBe('1');
});
