import { invoke } from "@tauri-apps/api/core";
import { telemetry } from "./telemetry";
import type { AppInfo, FileEntry, Highlight, Problem, Rect, RedactCandidate, RedactMode, SearchResult } from "../types";

export const api = {
  info: () => invoke<AppInfo>("app_info"),
  loadFolder: (folder: string) => invoke<AppInfo>("load_folder", { folder }),
  /** Detiene la carga, el análisis o el guardado en curso */
  cancelIndex: () => invoke<void>("cancel_index"),
  search: (query: string) => (telemetry.contar("busqueda"), invoke<SearchResult>("search", { query })),
  problems: () => invoke<Problem[]>("index_problems"),
  /** Con página, se abre en el navegador directamente en ella */
  openFile: (id: number, page?: number) => (telemetry.contar("abrir_documento"), invoke<void>("open_file", { id, page: page ?? null })),
  listFiles: () => invoke<FileEntry[]>("list_files"),
  viewPage: (id: number, page: number) => invoke<ArrayBuffer>("view_page", { id, page }),
  locate: (id: number, page: number, query: string) => invoke<Highlight[]>("locate_matches", { id, page, query }),
  revealFile: (id: number) => invoke<void>("reveal_file", { id }),
  redactionCandidates: (mode: RedactMode, texts: string[], fromFileName: boolean) =>
    invoke<RedactCandidate[]>("redaction_candidates", { mode, texts, fromFileName }),
  redactionDefaultOutput: () => invoke<string>("redaction_default_output"),
  /** Los documentos llegan con el evento redact-doc; el avance con redact-progress */
  startAnalysis: (args: { mode: RedactMode; texts: string[]; fromFileName: boolean; fileIds: number[] }) =>
    (telemetry.testado("analisis"), invoke<void>("start_analysis", args)),
  pageImage: (id: number, page: number) => invoke<ArrayBuffer>("redaction_page_image", { id, page }),
  applyRedaction: (outputDir: string, docs: { id: number; pages: { page: number; rects: Rect[] }[] }[]) =>
    (telemetry.testado("guardado"), invoke<void>("apply_redaction", { outputDir, docs })),
  /** Abre un PDF testado; con ruta vacía abre la carpeta de salida */
  openRedacted: (path: string) => invoke<void>("open_redacted", { path }),
};
