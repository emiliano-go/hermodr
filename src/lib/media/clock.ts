import { formatNumber } from "../i18n/localizer.ts";

export function mediaClock(seconds: number, hours = false): string {
  const value = Number.isFinite(seconds) ? Math.max(0, Math.floor(seconds)) : 0;
  const number = (part: number, minimumIntegerDigits = 1) => formatNumber(part, { useGrouping: false, minimumIntegerDigits, maximumFractionDigits: 0 });
  const h = Math.floor(value / 3600), m = Math.floor(value / 60), s = number(value % 60, 2);
  return hours && h ? `${number(h)}:${number(m % 60, 2)}:${s}` : `${number(m)}:${s}`;
}
