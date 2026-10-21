import { useCallback, useEffect, useLayoutEffect, useRef, useState, type ReactNode } from "react";
import { AlertTriangle, ChevronLeft, ChevronRight, ChevronsLeft, ChevronsRight, ExternalLink, FileSearch, FolderOpen, Loader2, Maximize, ZoomIn, ZoomOut } from "lucide-react";
import { Button } from "./ui/Button";
import { api } from "../lib/api";
import { fmt } from "../lib/format";
import type { FileEntry, Highlight, Hit } from "../types";

const ZOOMS = [1, 1.25, 1.5, 2, 2.5, 3, 4];
/** Imágenes de página que se conservan en memoria. */
const IMAGE_CACHE = 40;

interface ViewerProps {
  file: FileEntry | null;
  hit: Hit | undefined;
  page: number;
  /** La búsqueda que produjo los resultados actuales */
  query: string;
  onPage: (page: number) => void;
  onError: (title: string, e: unknown) => void;
}

/** Página del documento con la coincidencia resaltada. */
export function Viewer({ file, hit, page, query, onPage, onError }: ViewerProps) {
  const [zoomIdx, setZoomIdx] = useState(0);
  const [image, setImage] = useState<{ key: string; url: string } | null>(null);
  const [natural, setNatural] = useState<{ width: number; height: number } | null>(null);
  const [marks, setMarks] = useState<{ key: string; list: Highlight[] } | null>(null);
  const [locating, setLocating] = useState(false);
  const boxRef = useRef<HTMLDivElement>(null);
  const [fit, setFit] = useState({ width: 0, height: 0 });
  const cache = useRef(new Map<string, string>());
  const pending = useRef(new Map<string, Promise<string>>());

  useEffect(() => {
    const el = boxRef.current;
    if (!el) return;
    const ro = new ResizeObserver(([entry]) => setFit({ width: Math.max(0, entry.contentRect.width - 40), height: Math.max(0, entry.contentRect.height - 40) }));
    ro.observe(el);
    return () => ro.disconnect();
  }, [file === null]); // eslint-disable-line react-hooks/exhaustive-deps

  useEffect(() => () => cache.current.forEach(URL.revokeObjectURL), []);

  const load = useCallback((id: number, p: number) => {
    const key = `${id}:${p}`;
    const hit = cache.current.get(key);
    if (hit) return Promise.resolve(hit);
    const inFlight = pending.current.get(key);
    if (inFlight) return inFlight;
    const job = api
      .viewPage(id, p)
      .then((buf) => {
        const url = URL.createObjectURL(new Blob([buf], { type: "image/jpeg" }));
        cache.current.set(key, url);
        if (cache.current.size > IMAGE_CACHE) {
          const [oldest, old] = cache.current.entries().next().value as [string, string];
          cache.current.delete(oldest);
          URL.revokeObjectURL(old);
        }
        return url;
      })
      .finally(() => pending.current.delete(key));
    pending.current.set(key, job);
    return job;
  }, []);

  // La página actual y, de paso, la siguiente
  useEffect(() => {
    if (!file || page < 1) return;
    let alive = true;
    load(file.id, page)
      .then((url) => alive && setImage({ key: `${file.id}:${page}`, url }))
      .catch((e) => alive && onError("No se pudo mostrar la página", e));
    if (page < file.pages) load(file.id, page + 1).catch(() => {});
    return () => {
      alive = false;
    };
  }, [file, page, load, onError]);

  const pageHit = hit?.pages.find((p) => p.page === page);
  const markKey = file && pageHit ? `${file.id}:${page}:${query}` : "";

  // Dónde está la coincidencia en la página (en las escaneadas, el OCR tarda un momento la primera vez)
  useEffect(() => {
    if (!file || !markKey) {
      setLocating(false);
      return;
    }
    let alive = true;
    setLocating(true);
    api
      .locate(file.id, page, query)
      .then((list) => alive && setMarks({ key: markKey, list }))
      .catch(() => alive && setMarks({ key: markKey, list: [] }))
      .finally(() => alive && setLocating(false));
    return () => {
      alive = false;
    };
  }, [file, page, query, markKey]);

  const highlights = marks && marks.key === markKey ? marks.list : [];
  const scale = natural ? Math.min(fit.width / natural.width, fit.height / natural.height) * ZOOMS[zoomIdx] : 0;
  const shown = natural && scale > 0 ? { width: Math.floor(natural.width * scale), height: Math.floor(natural.height * scale) } : undefined;

  // Con zoom, la vista se lleva a la primera coincidencia
  useLayoutEffect(() => {
    const el = boxRef.current;
    const first = highlights[0]?.rects[0];
    if (!el || !shown || !first) return;
    const pad = 20;
    el.scrollLeft = Math.max(0, pad + ((first[0] + first[2]) / 2) * shown.width - el.clientWidth / 2);
    el.scrollTop = Math.max(0, pad + ((first[1] + first[3]) / 2) * shown.height - el.clientHeight / 2);
  }, [highlights, shown?.width, shown?.height]); // eslint-disable-line react-hooks/exhaustive-deps

  const hitPages = hit?.pages.map((p) => p.page) ?? [];
  const prevHit = [...hitPages].reverse().find((p) => p < page);
  const nextHit = hitPages.find((p) => p > page);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      const tag = (e.target as HTMLElement)?.tagName;
      if (!file || tag === "INPUT" || tag === "TEXTAREA") return;
      if (e.key === "ArrowRight" && page < file.pages) onPage(page + 1);
      else if (e.key === "ArrowLeft" && page > 1) onPage(page - 1);
      else if (e.key === "+" || e.key === "=") setZoomIdx((z) => Math.min(ZOOMS.length - 1, z + 1));
      else if (e.key === "-") setZoomIdx((z) => Math.max(0, z - 1));
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [file, page, onPage]);

  if (!file) {
    return (
      <section className="flex min-w-0 flex-1 flex-col items-center justify-center gap-3 rounded-2xl border border-white/[0.08] bg-panel px-6 text-center text-faint">
        <FileSearch size={28} />
        <span className="max-w-sm text-[13px] text-muted">Elige un documento de la lista o un resultado de la búsqueda para verlo aquí.</span>
      </section>
    );
  }

  const approxText = highlights.find((h) => !h.exact)?.text;
  return (
    <section className="flex min-w-0 flex-1 flex-col overflow-hidden rounded-2xl border border-white/[0.08] bg-panel">
      <div className="flex shrink-0 flex-wrap items-center gap-1.5 border-b border-white/[0.06] px-3 py-2">
        <IconButton label="Coincidencia anterior" disabled={prevHit === undefined} onClick={() => prevHit && onPage(prevHit)} icon={<ChevronsLeft size={14} />} />
        <IconButton label="Página anterior" disabled={page <= 1} onClick={() => onPage(page - 1)} icon={<ChevronLeft size={14} />} />
        <span className="min-w-[104px] text-center text-xs text-ink2">
          Página {fmt(page)} de {fmt(file.pages)}
        </span>
        <IconButton label="Página siguiente" disabled={page >= file.pages} onClick={() => onPage(page + 1)} icon={<ChevronRight size={14} />} />
        <IconButton label="Coincidencia siguiente" disabled={nextHit === undefined} onClick={() => nextHit && onPage(nextHit)} icon={<ChevronsRight size={14} />} />
        <span className="mx-1.5 h-5 w-px bg-white/10" />
        <IconButton label="Alejar" disabled={zoomIdx === 0} onClick={() => setZoomIdx((z) => Math.max(0, z - 1))} icon={<ZoomOut size={14} />} />
        <button
          onClick={() => setZoomIdx(0)}
          title="Ajustar la página a la ventana"
          className="flex h-8 min-w-[52px] items-center justify-center gap-1 rounded-lg border border-white/[0.12] px-2 text-[11px] text-muted hover:border-white/25 hover:text-ink"
        >
          {zoomIdx === 0 ? <Maximize size={12} /> : `${Math.round(ZOOMS[zoomIdx] * 100)}%`}
        </button>
        <IconButton label="Acercar" disabled={zoomIdx === ZOOMS.length - 1} onClick={() => setZoomIdx((z) => Math.min(ZOOMS.length - 1, z + 1))} icon={<ZoomIn size={14} />} />
        <span className="flex-1" />
        <IconButton label="Mostrar en la carpeta" onClick={() => api.revealFile(file.id).catch((e) => onError("No se pudo mostrar el documento", e))} icon={<FolderOpen size={14} />} />
        <Button variant="gold" size="sm" icon={<ExternalLink size={13} />} onClick={() => api.openFile(file.id, page).catch((e) => onError("No se pudo abrir el documento", e))} title="Abre el PDF en el navegador, en esta página">
          Abrir en pág. {fmt(page)}
        </Button>
      </div>

      <div className="flex min-h-[34px] shrink-0 items-center gap-2 border-b border-white/[0.05] px-4 py-1.5 text-[11.5px]">
        <span className="min-w-0 flex-1 truncate text-ink2" title={file.rel}>
          {file.name}
        </span>
        {locating ? (
          <span className="flex shrink-0 items-center gap-1.5 text-muted">
            <Loader2 size={12} className="animate-spin text-gold" /> Ubicando la coincidencia…
          </span>
        ) : pageHit && !pageHit.exact ? (
          <span className="flex min-w-0 shrink items-center gap-1.5 text-warn" title={approxText}>
            <AlertTriangle size={12} className="shrink-0" />
            <span className="truncate">Coincidencia aproximada{approxText ? `: se leyó «${approxText}»` : ""}. Valídala.</span>
          </span>
        ) : pageHit ? (
          <span className="shrink-0 text-goldtxt">{highlights.length > 1 ? `${highlights.length} coincidencias en esta página` : "Coincidencia exacta"}</span>
        ) : hit && hit.pages_matched > 0 ? (
          <span className="shrink-0 text-faint">Sin coincidencias en esta página</span>
        ) : null}
      </div>

      <div ref={boxRef} className="relative flex min-h-0 flex-1 overflow-auto bg-ground/60">
        <div className="relative m-auto shrink-0 p-5" style={shown ? { width: shown.width + 40, height: shown.height + 40 } : undefined}>
          {image && (
            <img
              src={image.url}
              alt={`Página ${page} de ${file.name}`}
              draggable={false}
              onLoad={(e) => setNatural({ width: e.currentTarget.naturalWidth, height: e.currentTarget.naturalHeight })}
              style={shown}
              className="block rounded-sm bg-white shadow-2xl"
            />
          )}
          {image && shown && highlights.length > 0 && (
            <svg viewBox="0 0 1000 1000" preserveAspectRatio="none" className="pointer-events-none absolute left-5 top-5" style={shown} aria-hidden>
              {highlights.flatMap((h, i) =>
                h.rects.map((r, j) => (
                  <rect
                    key={`${i}-${j}`}
                    x={r[0] * 1000}
                    y={r[1] * 1000}
                    width={(r[2] - r[0]) * 1000}
                    height={(r[3] - r[1]) * 1000}
                    rx={4}
                    fill={h.exact ? "#BC955C" : "#E8C06A"}
                    fillOpacity={0.28}
                    stroke={h.exact ? "#BC955C" : "#E8C06A"}
                    strokeWidth={2}
                    strokeDasharray={h.exact ? undefined : "6 4"}
                    vectorEffect="non-scaling-stroke"
                  />
                ))
              )}
            </svg>
          )}
          {image && image.key !== `${file.id}:${page}` && (
            <div className="absolute inset-5 flex items-center justify-center bg-ground/40">
              <Loader2 size={22} className="animate-spin text-gold" />
            </div>
          )}
          {!image && (
            <div className="flex h-[50vh] w-[60vh] max-w-full items-center justify-center">
              <Loader2 size={22} className="animate-spin text-gold" />
            </div>
          )}
        </div>
      </div>
    </section>
  );
}

function IconButton({ label, icon, onClick, disabled }: { label: string; icon: ReactNode; onClick: () => void; disabled?: boolean }) {
  return (
    <button
      aria-label={label}
      title={label}
      onClick={onClick}
      disabled={disabled}
      className="flex h-8 w-8 items-center justify-center rounded-lg border border-white/[0.12] text-muted transition-colors hover:border-white/25 hover:text-ink disabled:cursor-not-allowed disabled:opacity-35 disabled:hover:border-white/[0.12] disabled:hover:text-muted"
    >
      {icon}
    </button>
  );
}
