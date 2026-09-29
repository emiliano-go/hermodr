type Press = { x: number; y: number; background: boolean };

/**
 * Dismisses an overlay only on a click that began on the background and did not move, so zooming,
 * panning, dragging and the overlay's own controls never close it. Handlers go on the outermost
 * element of the overlay: `down` on `pointerdown` and `click` on `click`, closing when it returns
 * true. The `click` gesture already excludes drags; the distance check keeps a press that started
 * on the media and ended on the background (or vice versa) from counting.
 */
export function backgroundPress(isBackground: (target: EventTarget | null) => boolean) {
  let press: Press | null = null;

  return {
    down(e: PointerEvent) {
      press = { x: e.clientX, y: e.clientY, background: isBackground(e.target) };
    },
    click(e: MouseEvent) {
      const origin = press;
      press = null;
      // A keyboard click has no pointer press behind it and never dismisses.
      if (!origin || e.detail === 0) return false;
      if (!origin.background) return false;
      return Math.abs(e.clientX - origin.x) + Math.abs(e.clientY - origin.y) <= 3;
    },
  };
}