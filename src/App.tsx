import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { AlertTriangle, EyeOff, Search, X } from "lucide-react";
import { TitleBar } from "./components/TitleBar";
import { StatusBadge, type Readiness } from "./components/StatusBadge";
import { FolderPanel } from "./components/FolderPanel";
import { SearchResults } from "./components/SearchResults";
import { DocumentPanel } from "./components/DocumentPanel";
import { Viewer } from "./components/Viewer";
import { RedactWorkspace } from "./components/RedactWorkspace";
import { Button } from "./components/ui/Button";
import { ToastContainer, useToasts } from "./components/Toast";
import { api } from "./lib/api";
import { fmt, fmtDuration } from "./lib/format";
import type { AppInfo, FileEntry, Hit, Progress, RedactProgress, SearchResult } from "./types";

const DEBOUNCE_MS = 160;

export default function App() {
  const [info, setInfo] = useState<AppInfo | null>(null);
  const [query, setQuery] = useState("");
  /** Resultado y la consulta que lo produjo */
  const [result, setResult] = useState<{ query: string; data: SearchResult } | null>(null);
  const [files, setFiles] = useState<FileEntry[]>([]);
  const [selected, setSelected] = useState<{ id: number; page: number } | null>(null);
  const [redactInitial, setRedactInitial] = useState<string[] | null>(null);
  const { toasts, add: toast, remove: removeToast } = useToasts();
  const searchSeq = useRef(0);
  const queryRef = useRef(query);
  queryRef.current = query;
  const inputRef = useRef<HTMLInputElement>(null);

  const fail = useCallback((title: string) => (e: unknown) => toast({ type: "error", title, message: String(e) }), [toast]);
  const onViewerError = useCallback((title: string, e: unknown) => toast({ type: "error", title, message: String(e) }), [toast]);

  const refresh = useCallback(() => api.info().then(setInfo).catch(fail("No se pudo leer el estado")), [fail]);

  const runSearch = useCallback(
    (q: string) => {
      const seq = ++searchSeq.current;
      if (q.trim().length < 2) {
        setResult(null);
        return;
      }
      api
        .search(q)
        .then((data) => seq === searchSeq.current && setResult({ query: q, data }))
        .catch(fail("No se pudo buscar"));
    },
    [fail]
  );

  useEffect(() => {
    refresh();
  }, [refresh]);

  useEffect(() => {
    const id = window.setTimeout(() => runSearch(query), DEBOUNCE_MS);
    return () => window.clearTimeout(id);
  }, [query, runSearch]);

  // Avance de la carga
  useEffect(() => {
    const un = listen<Progress>("index-progress", ({ payload: p }) => {
      const finished = p.phase === "done" || p.phase === "cancelled" || p.phase === "error";
      if (!finished) {
        setInfo((prev) => (prev ? { ...prev, indexing: true, progress: p } : prev));
        return;
      }
      refresh();
      runSearch(queryRef.current);
      if (p.phase === "done") {
        // Los archivos ilegibles se reintentan en cada carga; no cuentan como nuevos
        const leidos = p.done - p.errors;
        const nuevos = leidos > 0 ? `${fmt(leidos)} documentos procesados en ${fmtDuration(p.elapsed_ms / 1000)}.` : "No había documentos nuevos ni modificados.";
        toast({ type: p.errors > 0 ? "warning" : "success", title: "Carga terminada", message: p.errors > 0 ? `${nuevos} ${fmt(p.errors)} no se pudieron leer.` : nuevos });
        inputRef.current?.focus();
      } else if (p.phase === "cancelled") {
        toast({ type: "warning", title: "Carga detenida", message: "Lo ya procesado se conserva. Presiona Actualizar PDFs para continuar." });
      } else {
        toast({ type: "error", title: "La carga falló", message: p.message });
      }
    });
    return () => {
      un.then((f) => f());
    };
  }, [refresh, runSearch, toast]);

  // Mientras se testa no se puede volver a cargar la carpeta
  useEffect(() => {
    const un = listen<RedactProgress>("redact-progress", ({ payload: p }) => {
      if (p.phase === "analyzing" || p.phase === "saving") setInfo((prev) => (prev && !prev.redacting ? { ...prev, redacting: true } : prev));
      else refresh();
    });
    return () => {
      un.then((f) => f());
    };
  }, [refresh]);

  const closeRedact = useCallback(() => setRedactInitial(null), []);

  // Durante la carga, los totales del índice se actualizan cada pocos segundos
  const indexing = info?.indexing ?? false;
  useEffect(() => {
    if (!indexing) return;
    const id = window.setInterval(() => api.info().then((i) => setInfo((prev) => (prev ? { ...prev, stats: i.stats } : i))), 3000);
    return () => window.clearInterval(id);
  }, [indexing]);

  // Lista de documentos: se relee cuando cambia la carpeta o la cantidad cargada
  const fileCount = info?.stats?.files ?? 0;
  useEffect(() => {
    api
      .listFiles()
      .then((list) => {
        setFiles(list);
        setSelected((s) => (s && list.some((f) => f.id === s.id) ? s : null));
      })
      .catch(fail("No se pudo leer la lista de documentos"));
  }, [info?.folder, fileCount, indexing, fail]);

  const load = (folder: string) => {
    setResult(null);
    setSelected(null);
    api.loadFolder(folder).then(setInfo).catch(fail("No se pudo cargar la carpeta"));
  };

  const hasDocs = fileCount > 0;
  const readiness: Readiness = indexing ? "loading" : hasDocs ? "ready" : "empty";
  const showing = result && query.trim().length >= 2 ? result : null;
  const hits = useMemo(() => new Map((showing?.data.hits ?? []).map((h) => [h.id, h])), [showing]);
  const selectedFile = selected ? files.find((f) => f.id === selected.id) ?? null : null;
  const onlyApprox = !!showing && showing.data.hits.length > 0 && showing.data.hits.every((h) => h.exact_pages === 0 && h.kind !== "name");

  // Al elegir un documento se abre en su primera coincidencia
  const open = (id: number) => setSelected({ id, page: hits.get(id)?.pages[0]?.page ?? 1 });
  const setPage = useCallback((page: number) => setSelected((s) => (s ? { ...s, page } : s)), []);

  return (
    <div className="flex h-screen flex-col overflow-hidden bg-ground">
      <TitleBar badge={<StatusBadge state={readiness} />} />
      <main className="flex min-h-0 flex-1 flex-col gap-3.5 px-6 py-4">
        <FolderPanel info={info} onLoad={load} onCancel={() => api.cancelIndex()} />

        <div className="grid h-[136px] shrink-0 grid-cols-2 gap-4">
          <div className="flex min-w-0 flex-col gap-1.5">
            <div
              className={`flex h-[48px] shrink-0 items-center gap-3 rounded-2xl border bg-panel px-4 transition-colors focus-within:border-gold/70 ${
                hasDocs ? "border-white/[0.14]" : "border-white/[0.06] opacity-60"
              }`}
            >
              <Search size={18} className="shrink-0 text-gold" />
              <label htmlFor="busqueda" className="sr-only">
                Buscar en los documentos
              </label>
              <input
                id="busqueda"
                ref={inputRef}
                autoFocus
                value={query}
                onChange={(e) => setQuery(e.target.value)}
                onKeyDown={(e) => {
                  if (e.key === "Enter") runSearch(query);
                  if (e.key === "Escape") setQuery("");
                }}
                disabled={!hasDocs}
                spellCheck={false}
                placeholder={hasDocs ? "Busca un nombre, un número de empleado, un folio o una frase" : "Carga una carpeta para habilitar la búsqueda"}
                className="min-w-0 flex-1 bg-transparent text-[14.5px] text-ink placeholder:text-faint focus:outline-none"
              />
              {query && (
                <button aria-label="Limpiar búsqueda" onClick={() => (setQuery(""), inputRef.current?.focus())} className="text-faint hover:text-ink">
                  <X size={16} />
                </button>
              )}
            </div>
            <div className="flex min-h-8 items-center gap-2 px-1 text-xs text-muted" aria-live="polite">
              {showing && (
                <span className="shrink-0">
                  <span className="font-medium text-ink2">{fmt(showing.data.total_files)}</span> {showing.data.total_files === 1 ? "documento" : "documentos"} ·{" "}
                  {showing.data.elapsed_ms < 1 ? "<1" : fmt(Math.round(showing.data.elapsed_ms))} ms
                </span>
              )}
              <span className="flex-1" />
              {hasDocs && (
                <Button
                  variant="danger"
                  size="sm"
                  icon={<EyeOff size={13} />}
                  disabled={indexing || (info?.redacting ?? false)}
                  title={indexing ? "Espera a que termine la carga" : "Testar los documentos de la carpeta"}
                  onClick={() => setRedactInitial(query.trim() ? [query.trim()] : [])}
                >
                  Testar documentos
                </Button>
              )}
            </div>
            <p className={`m-0 flex items-start gap-1.5 px-1 text-[11px] leading-snug ${onlyApprox ? "text-warn" : "text-faint"}`}>
              {onlyApprox && <AlertTriangle size={12} className="mt-px shrink-0" />}
              {onlyApprox
                ? "No hay coincidencias exactas: se muestran lecturas parecidas del OCR que conviene validar."
                : "Se toleran errores de lectura del OCR. Las páginas con coincidencia aproximada se marcan para validarlas."}
            </p>
          </div>

          <SearchResults query={query} result={showing?.data ?? null} ready={hasDocs} selected={selected?.id ?? null} onPick={(h: Hit) => open(h.id)} />
        </div>

        <div className="flex min-h-0 flex-1 gap-4">
          <DocumentPanel
            files={files}
            hits={hits}
            searching={!!showing}
            selected={selectedFile}
            page={selected?.page ?? 1}
            onSelect={(f) => open(f.id)}
            onPage={setPage}
          />
          <Viewer file={selectedFile} hit={selectedFile ? hits.get(selectedFile.id) : undefined} page={selected?.page ?? 1} query={showing?.query ?? ""} onPage={setPage} onError={onViewerError} />
        </div>
      </main>
      <RedactWorkspace initial={redactInitial} onClose={closeRedact} toast={toast} />
      <ToastContainer toasts={toasts} onRemove={removeToast} />
    </div>
  );
}
