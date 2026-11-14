import { afterEach, expect, it, vi } from 'vitest';
import { placePopover, scrollMovesAnchor } from './popover';

it('moves an anchor only for its own container, its ancestors and the document', () => {
  const chat = document.createElement('div');
  const anchor = document.createElement('button');
  const unrelated = document.createElement('div');
  chat.append(anchor);
  document.body.append(chat, unrelated);
  expect(scrollMovesAnchor(chat, anchor)).toBe(true);
  expect(scrollMovesAnchor(anchor, anchor)).toBe(true);
  expect(scrollMovesAnchor(document.body, anchor)).toBe(true);
  expect(scrollMovesAnchor(document, anchor)).toBe(true);
  expect(scrollMovesAnchor(window, anchor)).toBe(true);
  expect(scrollMovesAnchor(unrelated, anchor)).toBe(false);
  expect(scrollMovesAnchor(anchor, chat)).toBe(false);
});

it('without an anchor element, only a document scroll moves the popover', () => {
  const chat = document.createElement('div');
  document.body.append(chat);
  expect(scrollMovesAnchor(chat, null)).toBe(false);
  expect(scrollMovesAnchor(document, null)).toBe(true);
  expect(scrollMovesAnchor(window, null)).toBe(true);
});

afterEach(() => { document.body.innerHTML = ''; vi.restoreAllMocks(); });

/** A panel that is still scaled by its entrance animation: the rect is smaller than the layout box. */
function scaledPanel(layoutWidth: number, layoutHeight: number) {
  const panel = document.createElement('div');
  document.body.append(panel);
  const scale = 0.98;
  vi.spyOn(panel, 'getBoundingClientRect').mockReturnValue({ x: 0, y: 0, left: 0, top: 0, right: layoutWidth * scale, bottom: layoutHeight * scale, width: layoutWidth * scale, height: layoutHeight * scale, toJSON() {} });
  Object.defineProperty(panel, 'offsetWidth', { configurable: true, value: layoutWidth });
  Object.defineProperty(panel, 'offsetHeight', { configurable: true, value: layoutHeight });
  return panel;
}

it('places a point-anchored panel by its layout size, not by its scaled rect', () => {
  const panel = scaledPanel(200, 80);
  placePopover(panel, { x: 900, y: 100 });
  expect(panel.style.getPropertyValue('--popover-x')).toBe('700px');
  placePopover(panel, { x: 100, y: 740 });
  expect(panel.style.getPropertyValue('--popover-y')).toBe('660px');
});

it('flips a submenu to the left by the layout width of the submenu', () => {
  const panel = scaledPanel(200, 80);
  const anchor = document.createElement('button');
  document.body.append(anchor);
  vi.spyOn(anchor, 'getBoundingClientRect').mockReturnValue({ x: 700, y: 100, left: 700, top: 100, right: 900, bottom: 130, width: 200, height: 30, toJSON() {} });
  placePopover(panel, anchor, true);
  expect(panel.style.getPropertyValue('--popover-x')).toBe('496px');
  expect(panel.style.getPropertyValue('--popover-y')).toBe('100px');
});
