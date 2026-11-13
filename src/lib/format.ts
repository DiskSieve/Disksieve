// Formatting helpers for sizes, ages and dates.
// Shared by the real IPC layer and the browser mock fallback.

/** Formats a byte count using binary (1024) steps, e.g. `1.4 GB`. `signed` prefixes growth with `+`. */
export function formatBytes(b: number, signed = false): string {
  const sign = signed && b > 0 ? "+" : b < 0 ? "-" : "";
  const a = Math.abs(b);
  const units = ["B", "KB", "MB", "GB", "TB"];
  let i = 0, v = a;
  while (v >= 1024 && i < units.length - 1) { v /= 1024; i++; }
  return `${sign}${v.toFixed(i >= 3 ? 1 : i === 0 ? 0 : 1)} ${units[i]}`;
}

/** Formats an age given in whole days, e.g. `today` or `42d ago`. */
export const formatAge = (d: number) => (d === 0 ? "today" : `${d}d ago`);
