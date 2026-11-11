export type PopoverAnchor = { x: number; y: number } | HTMLElement;

/** Portal coordinates are viewport pixels, including when the application uses text zoom. */
export function placePopover(panel: HTMLElement, anchor: PopoverAnchor, side = false) {
  const rect = anchor instanceof HTMLElement ? anchor.getBoundingClientRect() : null;
  const { width, height } = panel.getBoundingClientRect();
  const gap = 4;
  let x = rect ? (side ? rect.right + gap : rect.left) : (anchor as { x: number }).x;
  let y = rect ? (side ? rect.top : rect.bottom + gap) : (anchor as { y: number }).y;
  if (x + width > window.innerWidth - 8) x = rect && side ? rect.left - width - gap : x - width;
  if (y + height > window.innerHeight - 8) y = rect && !side ? rect.top - height - gap : y - height;
  panel.style.setProperty('--popover-x', `${Math.max(8, Math.min(x, window.innerWidth - width - 8))}px`);
  panel.style.setProperty('--popover-y', `${Math.max(8, Math.min(y, window.innerHeight - height - 8))}px`);
}
