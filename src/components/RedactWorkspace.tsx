import { useCallback, useEffect, useMemo, useRef, useState, type ReactNode } from "react";
import { motion } from "motion/react";
import { listen } from "@tauri-apps/api/event";
import { open } from "@tauri-apps/plugin-dialog";
import {
  AlertTriangle,
  ArrowLeft,
  CheckCircle2,
  ChevronLeft,
  ChevronRight,
  Eye,
  EyeOff,
  FileText,
  FolderOpen,
  Highlighter,
  Loader2,
  MousePointerClick,
  RotateCcw,
  Rows3,
  ShieldCheck,
  Square,
  Undo2,
  X,
  XCircle,
} from "lucide-react";
import { Button } from "./ui/Button";
import { PageEditor, type Tool } from "./PageEditor";
import { api } from "../lib/api";
import { fmt, fmtDuration, pct } from "../lib/format";
import type { RedactCandidate, RedactDoc, RedactMode, RedactProgress, RedactRegion, Toast } from "../types";

type Step = "setup" | "analyzing" | "review" | "saving" | "done";

interface RedactWorkspaceProps {
  /** Textos con que se abre (la búsqueda actual); nulo cierra el espacio de trabajo */
  initial: string[] | null;
  onClose: () => void;
  toast: (t: Omit<Toast, "id">) => void;
}

/** Testado en cuatro pasos: qué testar → análisis → revisión página por página → guardado. */
export function RedactWorkspace({ initial, onClose, toast }: RedactWorkspaceProps) {
  const [step, setStep] = useState<Step>("setup");
  const [mode, setMode] = useState<RedactMode>("keep");
  const [texts, setTexts] = useState<string[]>([]);
  const [fromFileName, setFromFileName] = useState(true);
  const [candidates, setCandidates] = useState<RedactCandidate[]>([]);
  const [unchecked, setUnchecked] = useState<Set<number>>(new Set());
  const [loadingCandidates, setLoadingCandidates] = useState(false);
  const [docs, setDocs] = useState<RedactDoc[]>([]);
  const [progress, setProgress] = useState<RedactProgress | null>(null);
  const [output, setOutput] = useState("");
  const stepRef = useRef(step);
  stepRef.current = step;

  // Al abrir
  useEffect(() => {
    if (initial === null) return;
    setStep("setup");
    setMode("keep");
    setTexts(initial.filter((t) => t.trim()));
    setFromFileName(true);
    setUnchecked(new Set());
    setDocs([]);
    setProgress(null);
    api.redactionDefaultOutput().then(setOutput).catch(() => setOutput(""));
  }, [initial]);

  // Documentos a los que aplica
  const usesTexts = mode === "text" || !fromFileName;
  useEffect(() => {
    if (initial === null || step !== "setup") return;
    if (usesTexts && texts.length === 0) {
      setCandidates([]);
      return;
    }
    let alive = true;
    setLoadingCandidates(true);
    const id = window.setTimeout(() => {
      api
        .redactionCandidates(mode, texts, fromFileName)
        .then((c) => alive && setCandidates(c))
        .catch((e) => alive && toast({ type: "error", title: "No se pudieron buscar los documentos", message: String(e) }))
        .finally(() => alive && setLoadingCandidates(false));
    }, 250);
    return () => {
      alive = false;
      window.clearTimeout(id);
    };
  }, [initial, step, mode, texts, fromFileName, usesTexts, toast]);

  // Eventos del análisis y del guardado
  useEffect(() => {
    const unDoc = listen<RedactDoc>("redact-doc", ({ payload }) => setDocs((d) => [...d, payload]));
    const unProg = listen<RedactProgress>("redact-progress", ({ payload: p }) => {
      setProgress(p);
      if (p.phase === "analyzing" || p.phase === "saving") return;
      const s = stepRef.current;
      if (s === "analyzing") {
        if (p.phase === "error") {
          toast({ type: "error", title: "El análisis falló", message: p.message });
          setStep("setup");
        } else setStep("review");
      } else if (s === "saving") setStep("done");
    });
    return () => {
      unDoc.then((f) => f());
      unProg.then((f) => f());
    };
  }, [toast]);

  const selected = candidates.filter((c) => !unchecked.has(c.id));
  const totalPages = selected.reduce((s, c) => s + c.pages, 0);

  function analyze() {
    setDocs([]);
    setProgress({ phase: "analyzing", total_files: selected.length, done_files: 0, total_pages: totalPages, done_pages: 0, current: "", elapsed_ms: 0, message: "", report: null });
    setStep("analyzing");
    api.startAnalysis({ mode, texts, fromFileName, fileIds: selected.map((c) => c.id) }).catch((e) => {
      setStep("setup");
      toast({ type: "error", title: "No se pudo iniciar el análisis", message: String(e) });
    });
  }

  function save() {
    const payload = docs
      .filter((d) => !d.error)
      .map((d) => ({ id: d.id, pages: d.pages.map((p) => ({ page: p.page, rects: p.regions.filter((r) => r.active).flatMap((r) => r.rects) })) }));
    setProgress({ phase: "saving", total_files: payload.length, done_files: 0, total_pages: 0, done_pages: 0, current: "", elapsed_ms: 0, message: "", report: null });
    setStep("saving");
    api.applyRedaction(output, payload).catch((e) => {
      setStep("review");
      toast({ type: "error", title: "No se pudo guardar", message: String(e) });
    });
  }

  if (initial === null) return null;
  const busy = step === "analyzing" || step === "saving";

  return (
    <motion.div
      className="fixed inset-x-0 bottom-0 top-10 z-40 flex flex-col bg-ground"
      initial={{ opacity: 0, y: 10 }}
      animate={{ opacity: 1, y: 0 }}
      transition={{ duration: 0.2, ease: "easeOut" }}
      role="dialog"
      aria-modal="true"
      aria-labelledby="testado-title"
    >
      <header className="flex shrink-0 items-center gap-4 border-b border-white/[0.06] bg-side px-6 py-3.5">
        <div className="flex h-9 w-9 items-center justify-center rounded-xl bg-guinda/25 text-rose">
          <EyeOff size={17} />
        </div>
        <div className="flex flex-col">
          <span className="text-[10px] uppercase tracking-[0.12em] text-gold">Testado de documentos</span>
          <h2 id="testado-title" className="m-0 text-[15px] font-semibold text-ink">
            {mode === "keep" ? "Conservar solo la fila de la persona" : "Testar textos"}
          </h2>
        </div>
        <Steps step={step} />
        <div className="flex-1" />
        <button aria-label="Cerrar" onClick={onClose} disabled={busy} className="text-faint hover:text-ink disabled:opacity-30">
          <X size={18} />
        </button>
      </header>

      {step === "setup" && (
        <Setup
          mode={mode}
          setMode={setMode}
          texts={texts}
          setTexts={setTexts}
          fromFileName={fromFileName}
          setFromFileName={setFromFileName}
          candidates={candidates}
          unchecked={unchecked}
          setUnchecked={setUnchecked}
          loading={loadingCandidates}
          usesTexts={usesTexts}
          footer={
            <>
              <span className="flex-1 text-xs text-muted">
                {selected.length > 0
                  ? `${fmt(selected.length)} ${selected.length === 1 ? "documento" : "documentos"} · ${fmt(totalPages)} páginas · análisis estimado ~${fmtDuration(totalPages * 0.55)}`
                  : "Ningún documento seleccionado."}
              </span>
              <Button variant="quiet" onClick={onClose}>
                Cancelar
              </Button>
              <Button variant="primary" icon={<Rows3 size={15} />} disabled={selected.length === 0 || (mode === "text" && texts.length === 0)} onClick={analyze}>
                Analizar {fmt(selected.length)} {selected.length === 1 ? "documento" : "documentos"}
              </Button>
            </>
          }
        />
      )}

      {(step === "analyzing" || step === "saving") && progress && (
        <Running progress={progress} saving={step === "saving"} docs={docs} />
      )}

      {step === "review" && (
        <Review
          docs={docs}
          setDocs={setDocs}
          mode={mode}
          output={output}
          setOutput={setOutput}
          onBack={() => setStep("setup")}
          onSave={save}
          toast={toast}
        />
      )}

      {step === "done" && progress && <Done progress={progress} onClose={onClose} toast={toast} />}
    </motion.div>
  );
}

function Steps({ step }: { step: Step }) {
  const items: [Step[], string][] = [
    [["setup"], "Qué testar"],
    [["analyzing"], "Análisis"],
    [["review"], "Revisión"],
    [["saving", "done"], "Guardado"],
  ];
  const current = items.findIndex(([s]) => s.includes(step));
  return (
    <ol className="ml-6 flex items-center gap-2 text-[11px]">
      {items.map(([, label], i) => (
        <li key={label} className="flex items-center gap-2">
          <span
            className={`flex h-5 w-5 items-center justify-center rounded-full text-[10px] font-semibold ${
              i < current ? "bg-jade/40 text-mint" : i === current ? "bg-gold text-ground" : "bg-white/[0.06] text-faint"
            }`}
          >
            {i + 1}
          </span>
          <span className={i === current ? "text-ink" : "text-faint"}>{label}</span>
          {i < items.length - 1 && <span className="mx-1 h-px w-5 bg-white/10" />}
        </li>
      ))}
    </ol>
  );
}

// ── Paso 1: qué testar ──────────────────────────────────────────────────────

interface SetupProps {
  mode: RedactMode;
  setMode: (m: RedactMode) => void;
  texts: string[];
  setTexts: (t: string[]) => void;
  fromFileName: boolean;
  setFromFileName: (v: boolean) => void;
  candidates: RedactCandidate[];
  unchecked: Set<number>;
  setUnchecked: (s: Set<number>) => void;
  loading: boolean;
  usesTexts: boolean;
  footer: ReactNode;
}

function Setup(props: SetupProps) {
  const { mode, candidates, unchecked } = props;
  const [draft, setDraft] = useState("");
  const add = (raw: string) => {
    const parts = raw
      .split(/[\n;]+/)
      .map((s) => s.trim())
      .filter(Boolean);
    if (parts.length === 0) return;
    props.setTexts([...props.texts, ...parts.filter((p) => !props.texts.includes(p))]);
    setDraft("");
  };
  const allOn = candidates.length > 0 && unchecked.size === 0;

  return (
    <>
      <div className="grid min-h-0 flex-1 grid-cols-[minmax(0,1fr)_minmax(0,1.15fr)] gap-6 overflow-auto px-8 py-6">
        <div className="flex flex-col gap-5">
          <div className="flex flex-col gap-2.5">
            <span className="text-[11px] uppercase tracking-[0.06em] text-faint">Modo</span>
            <ModeCard
              active={mode === "keep"}
              onClick={() => props.setMode("keep")}
              icon={<Rows3 size={18} />}
              title="Conservar solo la fila de la persona"
              text="En cada hoja se testan todas las filas de la tabla, excepto la de la persona. El encabezado y el resto de la hoja quedan visibles."
            />
            <ModeCard
              active={mode === "text"}
              onClick={() => props.setMode("text")}
              icon={<Highlighter size={18} />}
              title="Testar textos"
              text="Se cubre cada aparición de los textos que indiques: nombres, números de empleado, palabras o frases."
            />
          </div>

          {mode === "keep" && (
            <label className="flex cursor-pointer items-start gap-3 rounded-xl border border-white/[0.08] bg-panel px-4 py-3.5">
              <input type="checkbox" checked={props.fromFileName} onChange={(e) => props.setFromFileName(e.target.checked)} className="mt-0.5 h-4 w-4 accent-[#BC955C]" />
              <span className="flex flex-col gap-1">
                <span className="text-[13px] text-ink">Reconocer a la persona por el nombre de cada archivo</span>
                <span className="text-xs leading-relaxed text-muted">
                  Del nombre «00002022195 ANDRES MARTINEZ FLORES junio.pdf» se toman el número de empleado y el nombre. Así cada documento conserva la fila de su propia persona.
                </span>
              </span>
            </label>
          )}

          {props.usesTexts && (
            <div className="flex flex-col gap-2">
              <label htmlFor="terminos" className="text-[11px] uppercase tracking-[0.06em] text-faint">
                {mode === "keep" ? "Datos de la persona cuya fila se conserva" : "Textos a testar"}
              </label>
              <div className="flex min-h-[44px] flex-wrap items-center gap-1.5 rounded-xl border border-white/[0.12] bg-panel px-2.5 py-2 focus-within:border-gold/60">
                {props.texts.map((t) => (
                  <span key={t} className="flex items-center gap-1.5 rounded-lg bg-gold/15 py-1 pl-2.5 pr-1.5 text-xs text-goldhi">
                    {t}
                    <button aria-label={`Quitar ${t}`} onClick={() => props.setTexts(props.texts.filter((x) => x !== t))} className="text-goldtxt/70 hover:text-goldhi">
                      <X size={12} />
                    </button>
                  </span>
                ))}
                <input
                  id="terminos"
                  value={draft}
                  onChange={(e) => setDraft(e.target.value)}
                  onKeyDown={(e) => {
                    if (e.key === "Enter") {
                      e.preventDefault();
                      add(draft);
                    } else if (e.key === "Backspace" && !draft && props.texts.length) props.setTexts(props.texts.slice(0, -1));
                  }}
                  onBlur={() => add(draft)}
                  onPaste={(e) => {
                    const t = e.clipboardData.getData("text");
                    if (/[\n;]/.test(t)) {
                      e.preventDefault();
                      add(t);
                    }
                  }}
                  placeholder={props.texts.length ? "Agregar otro…" : mode === "keep" ? "Nombre completo, número de empleado…" : "Un nombre, un número, una palabra… y Enter"}
                  className="min-w-[180px] flex-1 bg-transparent px-1.5 text-[13px] text-ink placeholder:text-faint focus:outline-none"
                />
              </div>
              <span className="text-[11px] text-faint">
                Presiona Enter después de cada uno. Puedes pegar una lista (un texto por renglón).
                {mode === "text" && " Se toleran errores de lectura del OCR en palabras y en números largos (un dígito)."}
              </span>
            </div>
          )}
        </div>

        <div className="flex min-h-[320px] flex-col overflow-hidden rounded-xl border border-white/[0.08] bg-panel">
          <div className="flex shrink-0 items-center gap-3 border-b border-white/[0.06] px-4 py-2.5">
            <input
              type="checkbox"
              aria-label="Seleccionar todos"
              checked={allOn}
              onChange={() => props.setUnchecked(allOn ? new Set(candidates.map((c) => c.id)) : new Set())}
              className="h-4 w-4 accent-[#BC955C]"
            />
            <span className="text-[11px] uppercase tracking-[0.06em] text-faint">Documentos ({fmt(candidates.length)})</span>
            {props.loading && <Loader2 size={13} className="animate-spin text-gold" />}
          </div>
          <div className="min-h-0 flex-1 overflow-auto">
            {candidates.length === 0 && !props.loading && (
              <div className="px-6 py-10 text-center text-xs text-muted">
                {props.usesTexts && props.texts.length === 0 ? "Agrega al menos un texto para ver a qué documentos aplica." : "Ningún documento coincide."}
              </div>
            )}
            {candidates.map((c) => (
              <label key={c.id} className="flex cursor-pointer items-center gap-3 border-b border-white/[0.05] px-4 py-2.5 hover:bg-white/[0.03]">
                <input
                  type="checkbox"
                  checked={!unchecked.has(c.id)}
                  onChange={() => {
                    const next = new Set(unchecked);
                    if (next.has(c.id)) next.delete(c.id);
                    else next.add(c.id);
                    props.setUnchecked(next);
                  }}
                  className="h-4 w-4 shrink-0 accent-[#BC955C]"
                />
                <FileText size={15} className="shrink-0 text-gold" />
                <div className="flex min-w-0 flex-1 flex-col gap-1">
                  <span className="truncate text-[13px] text-ink" title={c.rel}>
                    {c.name}
                  </span>
                  {mode === "keep" && (
                    <span className="flex flex-wrap gap-1">
                      {c.identity.map((w) => (
                        <span key={w} className="rounded bg-jade/15 px-1.5 py-px font-mono text-[10.5px] text-jadetxt">
                          {w}
                        </span>
                      ))}
                    </span>
                  )}
                </div>
                <span className="shrink-0 text-[11px] text-faint">{fmt(c.pages)} págs.</span>
              </label>
            ))}
          </div>
        </div>
      </div>
      <footer className="flex shrink-0 items-center gap-3 border-t border-white/[0.06] bg-side px-8 py-4">{props.footer}</footer>
    </>
  );
}

function ModeCard({ active, onClick, icon, title, text }: { active: boolean; onClick: () => void; icon: ReactNode; title: string; text: string }) {
  return (
    <button
      onClick={onClick}
      aria-pressed={active}
      className={`flex items-start gap-3.5 rounded-xl border px-4 py-3.5 text-left transition-colors ${
        active ? "border-gold/60 bg-gold/[0.08]" : "border-white/[0.08] bg-panel hover:border-white/20"
      }`}
    >
      <span className={`mt-0.5 ${active ? "text-goldhi" : "text-faint"}`}>{icon}</span>
      <span className="flex flex-col gap-1">
        <span className={`text-[13px] font-medium ${active ? "text-ink" : "text-ink2"}`}>{title}</span>
        <span className="text-xs leading-relaxed text-muted">{text}</span>
      </span>
    </button>
  );
}

// ── Pasos 2 y 4: avance ─────────────────────────────────────────────────────

function Running({ progress: p, saving, docs }: { progress: RedactProgress; saving: boolean; docs: RedactDoc[] }) {
  const ratio = p.total_pages > 0 ? pct(p.done_pages, p.total_pages) : p.total_files > 0 ? pct(p.done_files, p.total_files) : 0;
  const remaining = p.done_pages > 2 ? ((p.elapsed_ms / 1000) * (p.total_pages - p.done_pages)) / p.done_pages : NaN;
  return (
    <div className="flex min-h-0 flex-1 flex-col items-center justify-center gap-5 px-8">
      <div className="flex w-full max-w-[640px] flex-col gap-4" aria-live="polite">
        <div className="flex items-center gap-3 text-sm text-ink">
          <Loader2 size={18} className="animate-spin text-gold" />
          {saving ? "Guardando los documentos testados…" : `Leyendo y analizando página ${fmt(Math.min(p.done_pages + 1, p.total_pages))} de ${fmt(p.total_pages)}`}
        </div>
        <div className="relative h-2 overflow-hidden rounded-full bg-white/[0.07]">
          <div className={`h-full rounded-full transition-[width] duration-300 ${saving ? "bg-guinda" : "bg-jade"}`} style={{ width: `${ratio}%` }} />
          <div className="absolute inset-y-0 left-0 w-1/3 animate-shimmer bg-gradient-to-r from-transparent via-white/20 to-transparent" />
        </div>
        <div className="flex items-center gap-3 text-xs text-muted">
          <span>
            {fmt(p.done_files)} de {fmt(p.total_files)} documentos
          </span>
          {isFinite(remaining) && remaining > 0 && <span>· faltan ~{fmtDuration(remaining)}</span>}
          <span className="min-w-0 flex-1 truncate text-right font-mono text-[11px] text-faint">{p.current}</span>
        </div>
        {!saving && docs.length > 0 && (
          <span className="text-xs text-faint">
            Listos para revisar: {fmt(docs.length)} · con páginas por revisar: {fmt(docs.filter((d) => d.error || d.pages.some((pg) => pg.status === "review")).length)}
          </span>
        )}
        <div className="flex justify-end">
          <Button variant="danger" size="sm" icon={<Square size={12} />} onClick={() => api.cancelIndex()}>
            Detener
          </Button>
        </div>
      </div>
    </div>
  );
}

// ── Paso 3: revisión ────────────────────────────────────────────────────────

interface ReviewProps {
  docs: RedactDoc[];
  setDocs: (f: (d: RedactDoc[]) => RedactDoc[]) => void;
  mode: RedactMode;
  output: string;
  setOutput: (v: string) => void;
  onBack: () => void;
  onSave: () => void;
  toast: (t: Omit<Toast, "id">) => void;
}

function Review({ docs, setDocs, mode, output, setOutput, onBack, onSave, toast }: ReviewProps) {
  const [docIdx, setDocIdx] = useState(0);
  const [pageIdx, setPageIdx] = useState(0);
  const [tool, setTool] = useState<Tool>("area");
  const [seeThrough, setSeeThrough] = useState(false);
  const [images, setImages] = useState<Record<string, string>>({});
  const [seen, setSeen] = useState<Set<string>>(new Set());
  const [history, setHistory] = useState<{ doc: number; page: number; regions: RedactRegion[] }[]>([]);
  const [original, setOriginal] = useState<Record<string, RedactRegion[]>>({});
  const loading = useRef(new Set<string>());
  const canvasRef = useRef<HTMLDivElement>(null);
  const [fit, setFit] = useState({ width: 0, height: 0 });
  useEffect(() => {
    const el = canvasRef.current;
    if (!el) return;
    // Área útil del lienzo (sin el relleno de 20 px por lado)
    const ro = new ResizeObserver(([entry]) => setFit({ width: Math.max(0, entry.contentRect.width), height: Math.max(0, entry.contentRect.height) }));
    ro.observe(el);
    return () => ro.disconnect();
  }, []);

  const doc = docs[docIdx];
  const page = doc?.pages[pageIdx];
  const key = (d: RedactDoc, p: number) => `${d.id}:${p}`;

  // Propuesta original de cada página, para poder restablecerla
  useEffect(() => {
    setOriginal((o) => {
      const next = { ...o };
      for (const d of docs) for (const p of d.pages) if (!(key(d, p.page) in next)) next[key(d, p.page)] = p.regions;
      return next;
    });
  }, [docs]);

  // Imágenes: la página actual y la siguiente
  const loadImage = useCallback(
    (d: RedactDoc | undefined, p: number | undefined) => {
      if (!d || p === undefined) return;
      const k = key(d, p);
      if (loading.current.has(k)) return;
      loading.current.add(k);
      api
        .pageImage(d.id, p)
        .then((buf) => setImages((m) => ({ ...m, [k]: URL.createObjectURL(new Blob([buf], { type: "image/jpeg" })) })))
        .catch((e) => {
          loading.current.delete(k);
          toast({ type: "error", title: "No se pudo mostrar la página", message: String(e) });
        });
    },
    [toast]
  );
  useEffect(() => {
    loadImage(doc, page?.page);
    loadImage(doc, doc?.pages[pageIdx + 1]?.page);
    if (doc && page) setSeen((s) => (s.has(key(doc, page.page)) ? s : new Set(s).add(key(doc, page.page))));
  }, [doc, page, pageIdx, loadImage]);
  useEffect(() => () => Object.values(images).forEach(URL.revokeObjectURL), []); // eslint-disable-line react-hooks/exhaustive-deps

  const setRegions = (regions: RedactRegion[]) => {
    if (!doc || !page) return;
    setHistory((h) => [...h.slice(-99), { doc: docIdx, page: pageIdx, regions: page.regions }]);
    setDocs((all) => all.map((d, i) => (i !== docIdx ? d : { ...d, pages: d.pages.map((p, j) => (j !== pageIdx ? p : { ...p, regions })) })));
  };
  const undo = () => {
    const last = history[history.length - 1];
    if (!last) return;
    setHistory((h) => h.slice(0, -1));
    setDocs((all) => all.map((d, i) => (i !== last.doc ? d : { ...d, pages: d.pages.map((p, j) => (j !== last.page ? p : { ...p, regions: last.regions })) })));
    setDocIdx(last.doc);
    setPageIdx(last.page);
  };

  const go = (delta: number) => {
    if (!doc) return;
    const next = pageIdx + delta;
    if (next >= 0 && next < doc.pages.length) setPageIdx(next);
    else if (delta > 0 && docIdx + 1 < docs.length) {
      setDocIdx(docIdx + 1);
      setPageIdx(0);
    } else if (delta < 0 && docIdx > 0) {
      setDocIdx(docIdx - 1);
      setPageIdx(docs[docIdx - 1].pages.length - 1);
    }
  };
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if ((e.target as HTMLElement)?.tagName === "INPUT") return;
      if (e.key === "ArrowRight") go(1);
      else if (e.key === "ArrowLeft") go(-1);
      else if (e.key.toLowerCase() === "z" && (e.ctrlKey || e.metaKey)) undo();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  });

  const pending = useMemo(() => {
    let n = 0;
    for (const d of docs) for (const p of d.pages) if (p.status === "review" && !seen.has(key(d, p.page))) n++;
    return n;
  }, [docs, seen]);
  const redactedPages = docs.reduce((s, d) => s + d.pages.filter((p) => p.regions.some((r) => r.active)).length, 0);
  const savable = docs.filter((d) => !d.error);

  if (!doc) {
    return (
      <div className="flex flex-1 flex-col items-center justify-center gap-4 text-sm text-muted">
        No se analizó ningún documento.
        <Button variant="ghost" onClick={onBack} icon={<ArrowLeft size={15} />}>
          Volver
        </Button>
      </div>
    );
  }

  return (
    <>
      <div className="flex min-h-0 flex-1">
        <aside className="flex w-[290px] shrink-0 flex-col border-r border-white/[0.06] bg-side">
          <div className="border-b border-white/[0.06] px-4 py-3 text-[11px] uppercase tracking-[0.06em] text-faint">Documentos ({fmt(docs.length)})</div>
          <div className="min-h-0 flex-1 overflow-auto">
            {docs.map((d, i) => {
              const review = d.pages.filter((p) => p.status === "review" && !seen.has(key(d, p.page))).length;
              const active = i === docIdx;
              return (
                <div key={d.id} className={`border-b border-white/[0.05] ${active ? "bg-gold/[0.07]" : ""}`}>
                  <button
                    onClick={() => {
                      setDocIdx(i);
                      setPageIdx(0);
                    }}
                    className="flex w-full items-start gap-2.5 px-4 py-3 text-left hover:bg-white/[0.03]"
                  >
                    {d.error ? (
                      <XCircle size={15} className="mt-0.5 shrink-0 text-rose" />
                    ) : review > 0 ? (
                      <AlertTriangle size={15} className="mt-0.5 shrink-0 text-warn" />
                    ) : (
                      <CheckCircle2 size={15} className="mt-0.5 shrink-0 text-mint" />
                    )}
                    <span className="flex min-w-0 flex-col gap-0.5">
                      <span className={`truncate text-[12.5px] ${active ? "text-goldhi" : "text-ink"}`} title={d.rel}>
                        {d.name}
                      </span>
                      <span className="text-[11px] text-faint">{d.error ? d.error : `${fmt(d.pages.length)} págs.${review > 0 ? ` · ${fmt(review)} por revisar` : ""}`}</span>
                    </span>
                  </button>
                  {active && !d.error && (
                    <div className="grid grid-cols-7 gap-1 px-4 pb-3">
                      {d.pages.map((p, j) => {
                        const isReview = p.status === "review" && !seen.has(key(d, p.page));
                        const cur = j === pageIdx;
                        return (
                          <button
                            key={p.page}
                            onClick={() => setPageIdx(j)}
                            aria-label={`Página ${p.page}`}
                            aria-current={cur ? "page" : undefined}
                            className={`h-7 rounded-md text-[11px] font-medium transition-colors ${
                              cur
                                ? "bg-gold text-ground"
                                : isReview
                                  ? "border border-warn/50 bg-warn/10 text-warn"
                                  : p.regions.some((r) => r.active)
                                    ? "bg-white/[0.07] text-ink2 hover:bg-white/[0.12]"
                                    : "border border-white/[0.08] text-faint hover:text-ink"
                            }`}
                          >
                            {p.page}
                          </button>
                        );
                      })}
                    </div>
                  )}
                </div>
              );
            })}
          </div>
        </aside>

        <section className="flex min-w-0 flex-1 flex-col">
          <div className="flex shrink-0 flex-wrap items-center gap-2 border-b border-white/[0.06] px-5 py-2.5">
            <Button variant="quiet" size="sm" onClick={() => go(-1)} icon={<ChevronLeft size={14} />} aria-label="Página anterior" />
            <span className="min-w-[120px] text-center text-xs text-ink2">
              Página {page?.page ?? "—"} de {fmt(doc.pages.length)}
            </span>
            <Button variant="quiet" size="sm" onClick={() => go(1)} icon={<ChevronRight size={14} />} aria-label="Página siguiente" />
            <span className="mx-2 h-5 w-px bg-white/10" />
            <ToolButton active={tool === "area"} onClick={() => setTool("area")} icon={<Highlighter size={14} />} label="Marcar área" />
            <ToolButton active={tool === "word"} onClick={() => setTool("word")} icon={<MousePointerClick size={14} />} label="Marcar palabra" />
            <span className="mx-2 h-5 w-px bg-white/10" />
            <ToolButton active={seeThrough} onClick={() => setSeeThrough((v) => !v)} icon={<Eye size={14} />} label="Ver debajo" />
            <Button variant="quiet" size="sm" onClick={undo} disabled={history.length === 0} icon={<Undo2 size={14} />}>
              Deshacer
            </Button>
            <Button
              variant="quiet"
              size="sm"
              disabled={!page}
              onClick={() => page && original[key(doc, page.page)] && setRegions(original[key(doc, page.page)])}
              icon={<RotateCcw size={14} />}
            >
              Restablecer página
            </Button>
          </div>

          {page?.note && (
            <div className="mx-5 mt-3 flex items-start gap-2 rounded-lg border border-warn/30 bg-warn/[0.08] px-3 py-2 text-xs text-warn">
              <AlertTriangle size={14} className="mt-px shrink-0" />
              {page.note}
            </div>
          )}

          <div ref={canvasRef} className="flex min-h-0 flex-1 items-center justify-center overflow-hidden p-5">
            {page && <PageEditor image={images[key(doc, page.page)] ?? null} page={page} tool={tool} seeThrough={seeThrough} fit={fit} onChange={setRegions} />}
          </div>

          <div className="flex shrink-0 items-center gap-4 border-t border-white/[0.06] px-5 py-2 text-[11px] text-faint">
            <Legend swatch="bg-black border border-white/20" text="Se testa (clic para dejar visible)" />
            {mode === "keep" && <Legend swatch="border-2 border-mint bg-mint/10" text="Fila de la persona" />}
            <Legend swatch="border border-dashed border-gold" text="Visible (clic para testar)" />
            <span className="flex-1" />
            <span>
              {tool === "area" ? "Arrastra sobre la página para cubrir una zona." : "Haz clic en una palabra para cubrirla."} ← → cambia de página · Ctrl+Z deshace
            </span>
          </div>
        </section>
      </div>

      <footer className="flex shrink-0 items-center gap-3 border-t border-white/[0.06] bg-side px-6 py-3.5">
        <Button variant="quiet" onClick={onBack} icon={<ArrowLeft size={15} />}>
          Cambiar selección
        </Button>
        <div className="flex h-10 min-w-0 flex-1 items-center gap-2.5 rounded-xl border border-white/[0.12] bg-ground px-3 focus-within:border-gold/60">
          <FolderOpen size={15} className="shrink-0 text-faint" />
          <input
            aria-label="Carpeta de salida"
            value={output}
            onChange={(e) => setOutput(e.target.value)}
            spellCheck={false}
            className="min-w-0 flex-1 bg-transparent font-mono text-xs text-ink focus:outline-none"
          />
          <button
            onClick={async () => {
              const picked = await open({ directory: true, multiple: false, defaultPath: output || undefined, title: "Carpeta para los documentos testados" });
              if (typeof picked === "string") setOutput(picked);
            }}
            className="text-xs text-goldtxt hover:text-goldhi"
          >
            Examinar
          </button>
        </div>
        {pending > 0 && (
          <span className="flex items-center gap-1.5 text-xs text-warn">
            <AlertTriangle size={13} /> {fmt(pending)} {pending === 1 ? "página sin revisar" : "páginas sin revisar"}
          </span>
        )}
        <Button variant="danger" icon={<ShieldCheck size={15} />} disabled={savable.length === 0 || !output.trim()} onClick={onSave}>
          Guardar {fmt(savable.length)} {savable.length === 1 ? "documento testado" : "documentos testados"}
          <span className="font-normal opacity-75">({fmt(redactedPages)} págs.)</span>
        </Button>
      </footer>
    </>
  );
}

function ToolButton({ active, onClick, icon, label }: { active: boolean; onClick: () => void; icon: ReactNode; label: string }) {
  return (
    <button
      onClick={onClick}
      aria-pressed={active}
      className={`flex h-8 items-center gap-1.5 rounded-lg border px-2.5 text-xs transition-colors ${
        active ? "border-gold/60 bg-gold/15 text-goldhi" : "border-white/[0.12] text-muted hover:border-white/25 hover:text-ink"
      }`}
    >
      {icon}
      {label}
    </button>
  );
}

function Legend({ swatch, text }: { swatch: string; text: string }) {
  return (
    <span className="flex items-center gap-1.5">
      <span className={`h-3 w-4 rounded-[3px] ${swatch}`} />
      {text}
    </span>
  );
}

// ── Resultado ───────────────────────────────────────────────────────────────

function Done({ progress: p, onClose, toast }: { progress: RedactProgress; onClose: () => void; toast: (t: Omit<Toast, "id">) => void }) {
  const files = p.report?.files ?? [];
  const ok = files.filter((f) => !f.error);
  const failed = files.length - ok.length;
  const openPath = (path: string) => api.openRedacted(path).catch((e) => toast({ type: "error", title: "No se pudo abrir", message: String(e) }));
  return (
    <>
      <div className="flex min-h-0 flex-1 flex-col gap-4 overflow-auto px-8 py-6">
        <div
          className={`flex items-start gap-3 rounded-xl border px-4 py-3.5 ${
            p.phase === "error" || failed > 0 ? "border-rose/35 bg-guinda/20 text-rose" : p.phase === "cancelled" ? "border-gold/30 bg-gold/10 text-warn" : "border-mint/25 bg-jade/15 text-mint"
          }`}
        >
          {p.phase === "error" || failed > 0 ? <XCircle size={18} className="mt-0.5" /> : <CheckCircle2 size={18} className="mt-0.5" />}
          <div className="flex flex-col gap-1">
            <span className="text-[13px] font-medium text-ink">
              {p.phase === "error"
                ? "No se pudo guardar"
                : `${p.phase === "cancelled" ? "Guardado detenido: " : ""}${fmt(ok.length)} ${ok.length === 1 ? "documento testado" : "documentos testados"}${failed > 0 ? ` · ${fmt(failed)} con error` : ""}`}
            </span>
            <span className="text-xs text-muted">
              {p.phase === "error" ? p.message : `Los originales no se modificaron. Las páginas testadas se guardaron como imagen, sin texto oculto. ${fmtDuration(p.elapsed_ms / 1000)}.`}
            </span>
          </div>
        </div>
        <div className="flex flex-col overflow-hidden rounded-xl border border-white/[0.08]">
          {files.map((f) => (
            <div key={f.rel} className="flex items-center gap-3 border-b border-white/[0.05] px-4 py-3 last:border-b-0">
              {f.error ? <XCircle size={16} className="shrink-0 text-rose" /> : <CheckCircle2 size={16} className="shrink-0 text-mint" />}
              <div className="flex min-w-0 flex-1 flex-col gap-0.5">
                <span className="truncate text-[13px] text-ink" title={f.rel}>
                  {f.rel}
                </span>
                <span className="text-[11px] text-faint">{f.error ?? `${fmt(f.pages)} ${f.pages === 1 ? "página testada" : "páginas testadas"}`}</span>
              </div>
              {f.output && (
                <Button variant="gold" size="sm" onClick={() => openPath(f.output)}>
                  Abrir
                </Button>
              )}
            </div>
          ))}
        </div>
      </div>
      <footer className="flex shrink-0 items-center justify-end gap-3 border-t border-white/[0.06] bg-side px-8 py-4">
        <Button variant="ghost" icon={<FolderOpen size={15} />} onClick={() => openPath("")}>
          Abrir carpeta de salida
        </Button>
        <Button variant="primary" onClick={onClose}>
          Listo
        </Button>
      </footer>
    </>
  );
}
