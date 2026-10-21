import { useEffect, useRef, useState, type PointerEvent } from "react";
import { Loader2 } from "lucide-react";
import type { Rect, RedactPage, RedactRegion } from "../types";

export type Tool = "area" | "word";

interface PageEditorProps {
  image: string | null;
  page: RedactPage;
  tool: Tool;
  /** Muestra las barras semitransparentes para ver lo que hay debajo */
  seeThrough: boolean;
  /** Espacio disponible en pantalla; la página se ajusta a él sin deformarse */
  fit: { width: number; height: number };
  onChange: (regions: RedactRegion[]) => void;
}

/** Arrastre mínimo (fracción de la página) para que cuente como área y no como clic. */
const MIN_DRAG = 0.006;
/** Margen alrededor de una palabra seleccionada, como fracción de su altura. */
const WORD_PAD_Y = 0.25;
const WORD_PAD_X = 0.15;

const inside = (r: Rect, x: number, y: number) => x >= r[0] && x <= r[2] && y >= r[1] && y <= r[3];

/** Página con las barras propuestas encima. Clic en una barra o fila la activa o desactiva; con
 * «Marcar área» se arrastra para cubrir una zona; con «Marcar palabra» un clic cubre la palabra. */
export function PageEditor({ image, page, tool, seeThrough, fit, onChange }: PageEditorProps) {
  const svgRef = useRef<SVGSVGElement>(null);
  const [drag, setDrag] = useState<{ x0: number; y0: number; x1: number; y1: number } | null>(null);
  const [hover, setHover] = useState<number | null>(null);
  const [hoverWord, setHoverWord] = useState<number | null>(null);
  const [natural, setNatural] = useState<{ width: number; height: number } | null>(null);
  const aspect = natural ? natural.height / natural.width : 1;
  const scale = natural ? Math.min(fit.width / natural.width, fit.height / natural.height) : 0;
  const shown = natural && scale > 0 ? { width: Math.floor(natural.width * scale), height: Math.floor(natural.height * scale) } : undefined;

  useEffect(() => setDrag(null), [page]);

  const point = (e: PointerEvent) => {
    const box = svgRef.current!.getBoundingClientRect();
    return { x: (e.clientX - box.left) / box.width, y: (e.clientY - box.top) / box.height };
  };
  const regionAt = (x: number, y: number) => {
    for (let i = page.regions.length - 1; i >= 0; i--) if (page.regions[i].rects.some((r) => inside(r, x, y))) return i;
    return null;
  };
  const wordAt = (x: number, y: number) => {
    const i = page.words.findIndex((w) => inside(w.rect, x, y));
    return i >= 0 ? i : null;
  };

  function onDown(e: PointerEvent) {
    if (e.button !== 0) return;
    (e.target as Element).setPointerCapture?.(e.pointerId);
    const p = point(e);
    setDrag({ x0: p.x, y0: p.y, x1: p.x, y1: p.y });
  }

  function onMove(e: PointerEvent) {
    const p = point(e);
    if (drag) {
      setDrag({ ...drag, x1: p.x, y1: p.y });
      return;
    }
    setHover(regionAt(p.x, p.y));
    setHoverWord(tool === "word" ? wordAt(p.x, p.y) : null);
  }

  function onUp() {
    if (!drag) return;
    const r: Rect = [Math.min(drag.x0, drag.x1), Math.min(drag.y0, drag.y1), Math.max(drag.x0, drag.x1), Math.max(drag.y0, drag.y1)];
    setDrag(null);
    const dragged = r[2] - r[0] > MIN_DRAG && r[3] - r[1] > MIN_DRAG;
    if (dragged && tool === "area") {
      onChange([...page.regions, { kind: "area", active: true, keep: false, label: "Área marcada a mano", rects: [r] }]);
      return;
    }
    if (dragged) return;
    const { x0: x, y0: y } = drag;
    const hit = regionAt(x, y);
    if (hit !== null) {
      const region = page.regions[hit];
      // Lo marcado a mano se quita; lo propuesto (filas, textos) se activa o desactiva
      if (region.kind === "area" || region.kind === "word") onChange(page.regions.filter((_, i) => i !== hit));
      else onChange(page.regions.map((reg, i) => (i === hit ? { ...reg, active: !reg.active } : reg)));
      return;
    }
    const w = tool === "word" ? wordAt(x, y) : null;
    if (w !== null) {
      const [x0, y0, x1, y1] = page.words[w].rect;
      const hgt = y1 - y0;
      const padded: Rect = [
        Math.max(0, x0 - hgt * WORD_PAD_X * aspect),
        Math.max(0, y0 - hgt * WORD_PAD_Y),
        Math.min(1, x1 + hgt * WORD_PAD_X * aspect),
        Math.min(1, y1 + hgt * WORD_PAD_Y),
      ];
      onChange([...page.regions, { kind: "word", active: true, keep: false, label: page.words[w].text, rects: [padded] }]);
    }
  }

  const k = 1000;
  const toBox = (r: Rect) => ({ x: r[0] * k, y: r[1] * k, width: (r[2] - r[0]) * k, height: (r[3] - r[1]) * k });
  const hovered = hover !== null ? page.regions[hover] : null;

  return (
    <div className="relative shrink-0 select-none" style={shown}>
      {image ? (
        <img
          src={image}
          alt={`Página ${page.page}`}
          draggable={false}
          onLoad={(e) => setNatural({ width: e.currentTarget.naturalWidth, height: e.currentTarget.naturalHeight })}
          className="block h-full w-full rounded-sm bg-white shadow-2xl"
        />
      ) : (
        <div className="flex h-[60vh] w-[80vh] max-w-full items-center justify-center rounded-sm bg-white/[0.04]">
          <Loader2 size={22} className="animate-spin text-gold" />
        </div>
      )}
      {image && (
        <svg
          ref={svgRef}
          viewBox={`0 0 ${k} ${k}`}
          preserveAspectRatio="none"
          className={`absolute inset-0 h-full w-full ${tool === "area" ? "cursor-crosshair" : "cursor-pointer"}`}
          onPointerDown={onDown}
          onPointerMove={onMove}
          onPointerUp={onUp}
          onPointerLeave={() => {
            setHover(null);
            setHoverWord(null);
          }}
        >
          {page.regions.map((reg, i) =>
            reg.rects.map((r, j) => {
              const b = toBox(r);
              const isHover = hover === i;
              if (reg.active)
                return (
                  <rect
                    key={`${i}-${j}`}
                    {...b}
                    fill="#000"
                    fillOpacity={seeThrough ? 0.45 : 1}
                    stroke={isHover ? "#EC8FA6" : "none"}
                    strokeWidth={2}
                    vectorEffect="non-scaling-stroke"
                  />
                );
              if (reg.keep)
                return <rect key={`${i}-${j}`} {...b} fill="#4FB58E" fillOpacity={isHover ? 0.16 : 0.08} stroke="#4FB58E" strokeWidth={2} vectorEffect="non-scaling-stroke" />;
              return (
                <rect
                  key={`${i}-${j}`}
                  {...b}
                  fill={isHover ? "#BC955C" : "none"}
                  fillOpacity={0.12}
                  stroke={isHover ? "#BC955C" : "none"}
                  strokeDasharray="6 4"
                  strokeWidth={1.5}
                  vectorEffect="non-scaling-stroke"
                />
              );
            })
          )}
          {hoverWord !== null && hover === null && (
            <rect {...toBox(page.words[hoverWord].rect)} fill="#BC955C" fillOpacity={0.25} stroke="#BC955C" strokeWidth={1.5} vectorEffect="non-scaling-stroke" />
          )}
          {drag && tool === "area" && (
            <rect
              {...toBox([Math.min(drag.x0, drag.x1), Math.min(drag.y0, drag.y1), Math.max(drag.x0, drag.x1), Math.max(drag.y0, drag.y1)])}
              fill="#9F2241"
              fillOpacity={0.35}
              stroke="#EC8FA6"
              strokeWidth={1.5}
              vectorEffect="non-scaling-stroke"
            />
          )}
        </svg>
      )}
      {hovered && !drag && (
        <div className="pointer-events-none absolute left-2 top-2 max-w-[70%] rounded-lg border border-white/10 bg-bar/95 px-2.5 py-1.5 text-[11px] text-ink2 shadow-lg">
          {hovered.active
            ? hovered.kind === "area" || hovered.kind === "word"
              ? "Clic para quitar este marcado"
              : "Clic para dejar visible"
            : hovered.keep
              ? "Fila de la persona (visible). Clic para testarla"
              : "Fila visible. Clic para testarla"}
        </div>
      )}
    </div>
  );
}
