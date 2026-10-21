export type Readiness = "empty" | "loading" | "ready";

/** Indicador de si el índice ya permite buscar. */
export function StatusBadge({ state }: { state: Readiness }) {
  if (state === "ready") {
    return (
      <div className="flex items-center gap-2 rounded-full border border-mint/25 bg-jade/15 px-2.5 py-1">
        <span className="h-[7px] w-[7px] animate-pulse-dot rounded-full bg-mint" />
        <span className="text-[11px] text-[#9ED8BF]">Listo para buscar</span>
      </div>
    );
  }
  if (state === "loading") {
    return (
      <div className="flex items-center gap-2 rounded-full border border-gold/30 bg-gold/10 px-2.5 py-1">
        <span className="h-[7px] w-[7px] animate-pulse rounded-full bg-gold" />
        <span className="text-[11px] text-goldhi">Cargando documentos…</span>
      </div>
    );
  }
  return (
    <div className="flex items-center gap-2 rounded-full border border-white/10 px-2.5 py-1">
      <span className="h-[7px] w-[7px] rounded-full bg-faint" />
      <span className="text-[11px] text-muted">Sin documentos cargados</span>
    </div>
  );
}
