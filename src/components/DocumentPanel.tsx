import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { AlertTriangle, FileText, XCircle } from "lucide-react";
import { fmt } from "../lib/format";
import type { FileEntry, Hit } from "../types";

/** Alto fijo de cada renglón: con él se dibujan solo los visibles aunque haya miles de archivos. */
const ROW = 46;
const OVERSCAN = 6;

interface DocumentPanelProps {
  files: FileEntry[];
  hits: Map<number, Hit>;
  searching: boolean;
  selected: FileEntry | null;
  page: number;
  onSelect: (file: FileEntry) => void;
  onPage: (page: number) => void;
}

/** Lista de documentos de la carpeta y, debajo, el mapa de páginas del documento elegido. */
export function DocumentPanel({ files, hits, searching, selected, page, onSelect, onPage }: DocumentPanelProps) {
  const [onlyHits, setOnlyHits] = useState(false);
  const shown = searching && onlyHits ? files.filter((f) => hits.has(f.id)) : files;

  return (
    <aside className="flex min-h-0 w-[310px] shrink-0 flex-col overflow-hidden rounded-2xl border border-white/[0.08] bg-panel">
      <div className="flex shrink-0 items-center gap-2 border-b border-white/[0.06] px-4 py-2.5">
        <span className="text-[11px] uppercase tracking-[0.06em] text-faint">Documentos ({fmt(shown.length)})</span>
        <span className="flex-1" />
        {searching && (
          <label className="flex cursor-pointer items-center gap-1.5 text-[11px] text-muted hover:text-ink">
            <input type="checkbox" checked={onlyHits} onChange={(e) => setOnlyHits(e.target.checked)} className="accent-[#BC955C]" />
            Solo con coincidencias
          </label>
        )}
      </div>
      <FileRows files={shown} hits={hits} selected={selected?.id ?? null} onSelect={onSelect} />
      {selected && <PageMap file={selected} hit={hits.get(selected.id)} page={page} onPage={onPage} />}
    </aside>
  );
}

function FileRows({ files, hits, selected, onSelect }: { files: FileEntry[]; hits: Map<number, Hit>; selected: number | null; onSelect: (f: FileEntry) => void }) {
  const ref = useRef<HTMLDivElement>(null);
  const [view, setView] = useState({ top: 0, height: 400 });

  useEffect(() => {
    const el = ref.current;
    if (!el) return;
    const ro = new ResizeObserver(() => setView({ top: el.scrollTop, height: el.clientHeight }));
    ro.observe(el);
    return () => ro.disconnect();
  }, []);

  // El documento elegido desde los resultados se desplaza a la vista
  useLayoutEffect(() => {
    const el = ref.current;
    const i = files.findIndex((f) => f.id === selected);
    if (!el || i < 0) return;
    const y = i * ROW;
    if (y < el.scrollTop || y + ROW > el.scrollTop + el.clientHeight) el.scrollTop = Math.max(0, y - el.clientHeight / 2 + ROW / 2);
  }, [selected, files]);

  const first = Math.max(0, Math.floor(view.top / ROW) - OVERSCAN);
  const last = Math.min(files.length, Math.ceil((view.top + view.height) / ROW) + OVERSCAN);

  return (
    <div ref={ref} onScroll={(e) => setView({ top: e.currentTarget.scrollTop, height: e.currentTarget.clientHeight })} className="min-h-[120px] flex-1 overflow-auto">
      {files.length === 0 ? (
        <div className="px-4 py-6 text-center text-xs text-faint">Sin documentos.</div>
      ) : (
        <div style={{ height: files.length * ROW, position: "relative" }}>
          {files.slice(first, last).map((f, n) => {
            const hit = hits.get(f.id);
            const active = f.id === selected;
            const approx = hit ? hit.pages_matched - hit.exact_pages : 0;
            return (
              <button
                key={f.id}
                onClick={() => onSelect(f)}
                aria-current={active ? "true" : undefined}
                style={{ position: "absolute", top: (first + n) * ROW, height: ROW, left: 0, right: 0 }}
                className={`flex items-center gap-2.5 border-b border-l-2 border-b-white/[0.04] px-3.5 text-left transition-colors ${
                  active ? "border-l-gold bg-gold/[0.08]" : "border-l-transparent hover:bg-white/[0.03]"
                }`}
              >
                {f.error ? <XCircle size={14} className="shrink-0 text-rose" /> : <FileText size={14} className={`shrink-0 ${hit ? "text-gold" : "text-faint"}`} />}
                <span className="flex min-w-0 flex-1 flex-col">
                  <span className={`truncate text-[12px] ${active ? "text-goldhi" : hit ? "text-ink" : "text-ink2"}`} title={f.rel}>
                    {f.name}
                  </span>
                  <span className="flex items-center gap-1.5 text-[10.5px] text-faint">
                    {f.error ? (
                      <span className="truncate text-rose/80" title={f.error}>
                        No se pudo leer
                      </span>
                    ) : (
                      <span>{fmt(f.pages)} págs.</span>
                    )}
                    {hit && hit.exact_pages > 0 && <span className="text-goldtxt">· {fmt(hit.exact_pages)} con coincidencia</span>}
                    {approx > 0 && <span className="text-warn">· {fmt(approx)} por validar</span>}
                    {hit?.kind === "name" && <span className="text-goldtxt">· coincide el nombre</span>}
                  </span>
                </span>
              </button>
            );
          })}
        </div>
      )}
    </div>
  );
}

function PageMap({ file, hit, page, onPage }: { file: FileEntry; hit: Hit | undefined; page: number; onPage: (p: number) => void }) {
  const marks = new Map((hit?.pages ?? []).map((p) => [p.page, p.exact]));
  const approx = (hit?.pages ?? []).filter((p) => !p.exact).length;
  return (
    <div className="flex max-h-[45%] shrink-0 flex-col border-t border-white/[0.08] bg-side">
      <div className="flex items-baseline gap-2 px-4 pb-2 pt-3">
        <span className="text-[11px] uppercase tracking-[0.06em] text-faint">Páginas</span>
        <span className="min-w-0 flex-1 truncate text-right text-[11px] text-muted" title={file.name}>
          {hit && hit.pages_matched > 0 ? `aparece en ${fmt(hit.pages_matched)} de ${fmt(file.pages)}` : `${fmt(file.pages)} en total`}
        </span>
      </div>
      <div className="min-h-0 overflow-auto px-4 pb-2">
        <div className="grid grid-cols-7 gap-1">
          {Array.from({ length: file.pages }, (_, i) => i + 1).map((p) => {
            const mark = marks.get(p);
            const cur = p === page;
            return (
              <button
                key={p}
                onClick={() => onPage(p)}
                aria-label={`Página ${p}${mark === true ? ", con coincidencia" : mark === false ? ", coincidencia por validar" : ""}`}
                aria-current={cur ? "page" : undefined}
                title={mark === false ? "Coincidencia aproximada: el OCR leyó el texto con errores. Conviene validarla." : mark ? "Coincidencia exacta" : undefined}
                className={`relative h-7 rounded-md text-[11px] font-medium transition-colors ${
                  cur
                    ? mark === false
                      ? "bg-warn text-ground"
                      : "bg-gold text-ground"
                    : mark === true
                      ? "border border-gold/60 bg-gold/15 text-goldhi hover:bg-gold/25"
                      : mark === false
                        ? "border border-dashed border-warn/70 bg-warn/10 text-warn hover:bg-warn/20"
                        : "border border-white/[0.08] text-faint hover:text-ink"
                }`}
              >
                {p}
                {mark === false && <AlertTriangle size={8} className={`absolute right-0.5 top-0.5 ${cur ? "text-ground" : "text-warn"}`} />}
              </button>
            );
          })}
        </div>
      </div>
      {hit && hit.pages_matched > 0 && (
        <div className="flex shrink-0 items-center gap-3 border-t border-white/[0.05] px-4 py-2 text-[10.5px] text-faint">
          <span className="flex items-center gap-1.5">
            <span className="h-2.5 w-2.5 rounded-[3px] border border-gold/60 bg-gold/15" /> Coincidencia
          </span>
          {approx > 0 && (
            <span className="flex items-center gap-1.5">
              <span className="h-2.5 w-2.5 rounded-[3px] border border-dashed border-warn/70 bg-warn/10" /> Por validar
            </span>
          )}
        </div>
      )}
    </div>
  );
}
