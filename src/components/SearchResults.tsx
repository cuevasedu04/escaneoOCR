import type { ReactNode } from "react";
import { FileText, Search, SearchX } from "lucide-react";
import { fmt } from "../lib/format";
import type { Hit, SearchResult } from "../types";

interface SearchResultsProps {
  query: string;
  result: SearchResult | null;
  ready: boolean;
  selected: number | null;
  onPick: (hit: Hit) => void;
}

/** Documentos donde aparece la búsqueda. Al elegir uno se abre en el visor. */
export function SearchResults({ query, result, ready, selected, onPick }: SearchResultsProps) {
  const showing = result && query.trim().length >= 2;
  return (
    <div className="flex h-full min-h-0 flex-col overflow-hidden rounded-2xl border border-white/[0.08] bg-panel">
      {!showing ? (
        <Empty icon={<Search size={18} />} text={ready ? "Los documentos donde aparezca lo que busques se listarán aquí." : "Carga una carpeta de documentos para poder buscar."} />
      ) : result.hits.length === 0 ? (
        <Empty icon={<SearchX size={18} />} text={`Ningún documento contiene «${query.trim()}».`} />
      ) : (
        <div className="min-h-0 flex-1 overflow-auto py-1">
          {result.hits.map((h) => {
            const approx = h.pages_matched - h.exact_pages;
            const active = h.id === selected;
            return (
              <button
                key={h.id}
                onClick={() => onPick(h)}
                aria-current={active ? "true" : undefined}
                className={`flex w-full flex-col gap-0.5 border-l-2 px-3.5 py-1.5 text-left transition-colors ${
                  active ? "border-gold bg-gold/[0.08]" : "border-transparent hover:bg-white/[0.04]"
                }`}
              >
                <span className="flex min-w-0 items-center gap-2">
                  <FileText size={13} className="shrink-0 text-gold" />
                  <span className={`min-w-0 flex-1 truncate text-[12.5px] font-medium ${active ? "text-goldhi" : "text-ink"}`} title={h.rel}>
                    {h.name}
                  </span>
                  {h.kind === "name" ? (
                    <Pill tone="muted">nombre del archivo</Pill>
                  ) : (
                    <>
                      {h.exact_pages > 0 && <Pill tone="gold">{`${fmt(h.exact_pages)} ${h.exact_pages === 1 ? "pág." : "págs."}`}</Pill>}
                      {approx > 0 && <Pill tone="warn">{`${fmt(approx)} por validar`}</Pill>}
                    </>
                  )}
                </span>
                <span className="truncate pl-[21px] text-[11.5px] text-muted">
                  <Snippet text={h.snippet} />
                </span>
              </button>
            );
          })}
          {result.total_files > result.hits.length && (
            <div className="px-3.5 py-2 text-center text-[11px] text-faint">
              Se muestran los {fmt(result.hits.length)} documentos más relevantes de {fmt(result.total_files)}. Agrega más palabras para afinar.
            </div>
          )}
        </div>
      )}
    </div>
  );
}

export function Snippet({ text }: { text: string }) {
  // Los fragmentos impares quedan entre los marcadores de coincidencia
  const parts = text.split(/[\u0001\u0002]/);
  return (
    <>
      {parts.map((part, i) =>
        i % 2 === 1 ? (
          <mark key={i} className="rounded-[3px] bg-gold/25 px-0.5 font-medium text-goldhi">
            {part}
          </mark>
        ) : (
          part
        )
      )}
    </>
  );
}

function Pill({ tone, children }: { tone: "gold" | "warn" | "muted"; children: string }) {
  const cls = tone === "gold" ? "border-gold/40 text-goldtxt" : tone === "warn" ? "border-warn/40 text-warn" : "border-white/[0.12] text-muted";
  return <span className={`shrink-0 rounded-full border px-1.5 py-px text-[10px] ${cls}`}>{children}</span>;
}

function Empty({ icon, text }: { icon: ReactNode; text: string }) {
  return (
    <div className="flex h-full flex-col items-center justify-center gap-2 px-6 text-center text-faint">
      {icon}
      <span className="max-w-sm text-xs text-muted">{text}</span>
    </div>
  );
}
