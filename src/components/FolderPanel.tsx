import { useEffect, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { AlertTriangle, CheckCircle2, ChevronDown, FolderOpen, FolderSearch, Square, Upload } from "lucide-react";
import { GlowCard } from "./ui/GlowCard";
import { Button } from "./ui/Button";
import { api } from "../lib/api";
import { fmt, fmtDuration, pct } from "../lib/format";
import type { AppInfo, Problem } from "../types";

interface FolderPanelProps {
  info: AppInfo | null;
  onLoad: (folder: string) => void;
  onCancel: () => void;
}

export function FolderPanel({ info, onLoad, onCancel }: FolderPanelProps) {
  const [path, setPath] = useState("");
  const indexing = info?.indexing ?? false;

  useEffect(() => {
    if (info?.folder) setPath((p) => p || info.folder!);
  }, [info?.folder]);

  async function browse() {
    const picked = await open({ directory: true, multiple: false, defaultPath: path || undefined, title: "Carpeta con los documentos PDF" });
    if (typeof picked === "string") setPath(picked);
  }

  const canLoad = path.trim().length > 0 && !indexing && !info?.redacting;
  const sameFolder = !!info?.folder && info.folder.toLowerCase() === path.trim().replace(/[\\/]+$/, "").toLowerCase();

  return (
    <GlowCard interactive={false} className="shrink-0">
      <div className="flex flex-col gap-2.5 px-5 py-3.5">
        <div className="flex items-center gap-2.5">
          <label htmlFor="carpeta" className="shrink-0 text-[11px] uppercase tracking-[0.06em] text-faint">
            Carpeta
          </label>
          <div className="flex h-10 min-w-0 flex-1 items-center gap-2.5 rounded-xl border border-white/[0.12] bg-ground px-3 focus-within:border-gold/60">
            <FolderSearch size={16} className="shrink-0 text-faint" />
            <input
              id="carpeta"
              value={path}
              onChange={(e) => setPath(e.target.value)}
              onKeyDown={(e) => e.key === "Enter" && canLoad && onLoad(path)}
              disabled={indexing}
              spellCheck={false}
              placeholder="Ejemplo: C:\Documentos\Expedientes"
              className="min-w-0 flex-1 bg-transparent font-mono text-xs text-ink placeholder:font-sans placeholder:text-faint focus:outline-none disabled:opacity-60"
            />
          </div>
          <Button variant="ghost" onClick={browse} disabled={indexing} icon={<FolderOpen size={15} />}>
            Examinar
          </Button>
          {indexing ? (
            <Button variant="danger" onClick={onCancel} icon={<Square size={13} />}>
              Detener
            </Button>
          ) : (
            <Button variant="primary" onClick={() => onLoad(path)} disabled={!canLoad} icon={<Upload size={15} />}>
              {sameFolder && (info?.stats?.files ?? 0) > 0 ? "Actualizar PDFs" : "Cargar PDFs"}
            </Button>
          )}
        </div>
        <Status info={info} />
      </div>
    </GlowCard>
  );
}

function Status({ info }: { info: AppInfo | null }) {
  const p = info?.progress;
  const stats = info?.stats;

  if (info?.indexing && p) {
    const ratio = p.total > 0 ? pct(p.done, p.total) : 0;
    const remaining = p.done > 5 ? ((p.elapsed_ms / 1000) * (p.total - p.done)) / p.done : NaN;
    const label =
      p.phase === "scanning"
        ? "Revisando la carpeta…"
        : p.phase === "optimizing"
          ? "Optimizando el índice para búsquedas más rápidas…"
          : p.total === 0
            ? "Sin archivos nuevos que procesar…"
            : `Procesando ${fmt(p.done)} de ${fmt(p.total)} archivos`;
    return (
      <div className="flex flex-col gap-2" aria-live="polite">
        <div className="relative h-1.5 overflow-hidden rounded-full bg-white/[0.07]">
          <div className="h-full rounded-full bg-jade transition-[width] duration-300" style={{ width: `${p.phase === "optimizing" ? 100 : ratio}%` }} />
          <div className="absolute inset-y-0 left-0 w-1/3 animate-shimmer bg-gradient-to-r from-transparent via-white/25 to-transparent" />
        </div>
        <div className="flex items-center gap-3 text-xs text-muted">
          <span className="text-ink2">{label}</span>
          {p.ocr_pages > 0 && <span>· {fmt(p.ocr_pages)} páginas leídas con OCR</span>}
          {p.unchanged > 0 && <span>· {fmt(p.unchanged)} ya estaban cargados</span>}
          {isFinite(remaining) && remaining > 0 && <span>· faltan ~{fmtDuration(remaining)}</span>}
          <span className="min-w-0 flex-1 truncate text-right font-mono text-[11px] text-faint" title={p.current}>
            {p.current}
          </span>
        </div>
        {(stats?.files ?? 0) > 0 && <span className="text-xs text-faint">Ya puedes buscar en los documentos procesados mientras termina la carga.</span>}
      </div>
    );
  }

  if (!stats || stats.files === 0) {
    return (
      <span className="text-xs text-muted">
        {info?.folder
          ? "Esta carpeta no tiene documentos cargados. Presiona Cargar PDFs."
          : "Indica la carpeta y presiona Cargar PDFs. La primera carga lee todos los documentos; las siguientes solo los nuevos o modificados."}
      </span>
    );
  }

  const problems = stats.errors + (stats.ocr_pending > 0 ? 1 : 0);
  return (
    <div className="flex flex-col gap-2">
      <div className="flex flex-wrap items-center gap-x-3 gap-y-1 text-xs text-muted">
        <span className="flex items-center gap-1.5 font-medium text-mint">
          <CheckCircle2 size={15} /> Listo para buscar
        </span>
        <span>
          {fmt(stats.files - stats.errors)} documentos · {fmt(stats.pages)} páginas
          {stats.ocr_pages > 0 && ` · ${fmt(stats.ocr_pages)} leídas con OCR`}
        </span>
        {info?.ocr_language === null && (
          <span className="flex items-center gap-1.5 text-warn">
            <AlertTriangle size={13} /> Este equipo no tiene OCR de Windows instalado: los documentos escaneados no se pueden leer.
          </span>
        )}
      </div>
      {problems > 0 && <Problems count={stats.errors} pending={stats.ocr_pending} />}
    </div>
  );
}

function Problems({ count, pending }: { count: number; pending: number }) {
  const [open, setOpen] = useState(false);
  const [rows, setRows] = useState<Problem[]>([]);

  useEffect(() => {
    if (open) api.problems().then(setRows).catch(() => setRows([]));
  }, [open, count, pending]);

  const parts = [];
  if (count > 0) parts.push(`${fmt(count)} archivos no se pudieron leer`);
  if (pending > 0) parts.push(`${fmt(pending)} páginas quedaron sin OCR`);

  return (
    <div className="flex flex-col gap-2">
      <button onClick={() => setOpen((o) => !o)} aria-expanded={open} className="flex items-center gap-1.5 self-start text-xs text-warn hover:text-goldhi">
        <AlertTriangle size={13} />
        {parts.join(" · ")}
        <ChevronDown size={13} className={`transition-transform ${open ? "rotate-180" : ""}`} />
      </button>
      {open && (
        <div className="max-h-32 overflow-auto rounded-lg border border-white/[0.08] bg-ground px-3 py-2">
          {rows.map((r) => (
            <div key={r.name} data-selectable className="flex gap-3 py-0.5 text-xs">
              <span className="min-w-0 flex-1 truncate font-mono text-ink2" title={r.name}>{r.name}</span>
              <span className="shrink-0 text-muted">{r.detail}</span>
            </div>
          ))}
        </div>
      )}
    </div>
  );
}
