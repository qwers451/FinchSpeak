export type PopoverAnchor = { x: number; y: number } | HTMLElement;

/**
 * A scroll moves an anchor only when it happens in the anchor or in one of its ancestors, or in the
 * document. Scrolling an unrelated list (chat, members) leaves the popover where it is.
 * A point anchor has no element to move with, so only a document scroll counts.
 */
export function scrollMovesAnchor(target: EventTarget | null, anchor: PopoverAnchor): boolean {
  if (!(target instanceof Element)) return true;
  if (!(anchor instanceof HTMLElement)) return false;
  return target.contains(anchor);
}

/**
 * Portal coordinates are viewport pixels, including when the application uses text zoom.
 * The panel's own size is its layout box (offsetWidth/offsetHeight), which transforms do not change: a panel
 * still scaled by its entrance animation (`@starting-style`) would otherwise be placed as if it were smaller.
 */
export function placePopover(panel: HTMLElement, anchor: PopoverAnchor, side = false) {
  const rect = anchor instanceof HTMLElement ? anchor.getBoundingClientRect() : null;
  const width = panel.offsetWidth;
  const height = panel.offsetHeight;
  const gap = 4;
  let x = rect ? (side ? rect.right + gap : rect.left) : (anchor as { x: number }).x;
  let y = rect ? (side ? rect.top : rect.bottom + gap) : (anchor as { y: number }).y;
  if (x + width > window.innerWidth - 8) x = rect && side ? rect.left - width - gap : x - width;
  if (y + height > window.innerHeight - 8) y = rect && !side ? rect.top - height - gap : y - height;
  panel.style.setProperty('--popover-x', `${Math.max(8, Math.min(x, window.innerWidth - width - 8))}px`);
  panel.style.setProperty('--popover-y', `${Math.max(8, Math.min(y, window.innerHeight - height - 8))}px`);
}
