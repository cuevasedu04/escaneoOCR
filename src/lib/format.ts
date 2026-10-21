const nf = new Intl.NumberFormat("es-MX");

export const fmt = (n: number | null | undefined) => (n == null ? "—" : nf.format(n));

export const pct = (part: number, total: number) => (total > 0 ? (part / total) * 100 : 0);

export function fmtDuration(seconds: number): string {
  if (!isFinite(seconds) || seconds < 0) return "";
  const m = Math.floor(seconds / 60);
  const s = Math.round(seconds % 60);
  return m > 0 ? `${m} min ${s} s` : `${s} s`;
}
