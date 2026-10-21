export interface Toast {
  id: number;
  type: "success" | "error" | "warning";
  title: string;
  message: string;
}

export interface Stats {
  files: number;
  pages: number;
  ocr_pages: number;
  ocr_pending: number;
  errors: number;
}

export interface Progress {
  phase: "scanning" | "indexing" | "optimizing" | "done" | "cancelled" | "error";
  total: number;
  done: number;
  unchanged: number;
  errors: number;
  ocr_pages: number;
  /** Documentos de esta carga con al menos una página leída con OCR */
  ocr_files?: number;
  current: string;
  elapsed_ms: number;
  message: string;
}

export interface AppInfo {
  folder: string | null;
  stats: Stats | null;
  indexing: boolean;
  redacting: boolean;
  progress: Progress | null;
  ocr_language: string | null;
}

export interface PageHit {
  page: number;
  /** false: coincidencia aproximada (errores de OCR) que conviene validar */
  exact: boolean;
}

export interface Hit {
  id: number;
  name: string;
  rel: string;
  /** Página del fragmento; 0 cuando coincide solo el nombre del archivo */
  page: number;
  /** La coincidencia viene delimitada por U+0001 … U+0002 */
  snippet: string;
  pages_matched: number;
  exact_pages: number;
  kind: "exact" | "approx" | "name";
  pages: PageHit[];
}

export interface SearchResult {
  hits: Hit[];
  total_files: number;
  elapsed_ms: number;
}

export interface FileEntry {
  id: number;
  name: string;
  rel: string;
  pages: number;
  error: string | null;
}

/** Lugar de una página donde aparece la búsqueda */
export interface Highlight {
  rects: Rect[];
  exact: boolean;
  /** Lo que se leyó ahí */
  text: string;
}

export interface Problem {
  name: string;
  detail: string;
}

/** [x0, y0, x1, y1] en fracciones del ancho y alto de la página */
export type Rect = [number, number, number, number];

/** keep: conservar solo la fila de la persona · text: testar los textos indicados */
export type RedactMode = "keep" | "text";

export interface RedactCandidate {
  id: number;
  name: string;
  rel: string;
  pages: number;
  /** Palabras con que se reconoce la fila de la persona */
  identity: string[];
}

export interface RedactRegion {
  /** row: fila de tabla · text: aparición de un texto · area/word: marcado a mano */
  kind: "row" | "text" | "area" | "word";
  /** Se testa. Las filas que quedan visibles llevan false */
  active: boolean;
  /** Fila reconocida como la de la persona */
  keep: boolean;
  label: string;
  rects: Rect[];
}

export interface RedactPage {
  page: number;
  status: "ok" | "review" | "empty";
  note: string;
  regions: RedactRegion[];
  words: { text: string; rect: Rect }[];
}

export interface RedactDoc {
  id: number;
  name: string;
  rel: string;
  pages: RedactPage[];
  error: string | null;
}

export interface RedactFileReport {
  name: string;
  rel: string;
  output: string;
  pages: number;
  error: string | null;
}

export interface RedactProgress {
  phase: "analyzing" | "saving" | "done" | "cancelled" | "error";
  total_files: number;
  done_files: number;
  total_pages: number;
  done_pages: number;
  current: string;
  elapsed_ms: number;
  message: string;
  report: { output_dir: string; files: RedactFileReport[] } | null;
}
