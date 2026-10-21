import type { ReactNode } from "react";
import { Minus, Square, X } from "lucide-react";
import { getCurrentWindow } from "@tauri-apps/api/window";

export function TitleBar({ badge }: { badge: ReactNode }) {
  const win = getCurrentWindow();
  const btn = "flex h-10 w-[46px] items-center justify-center text-muted transition-colors duration-100";

  return (
    <div data-tauri-drag-region className="flex h-10 shrink-0 items-center gap-3 border-b border-white/[0.06] bg-bar pl-4">
      <div data-tauri-drag-region className="pointer-events-none flex items-center gap-3">
        <span className="flex h-[18px] w-[18px] items-center justify-center rounded-[5px] bg-guinda">
          <span className="h-2 w-2 rounded-[2px] bg-gold" />
        </span>
        <span className="text-xs font-medium tracking-wide text-ink2">Búsqueda de Documentos · ANAM</span>
      </div>
      <div data-tauri-drag-region className="h-full flex-1" />
      {badge}
      <div className="ml-2 flex">
        <button aria-label="Minimizar" onClick={() => win.minimize()} className={`${btn} hover:bg-white/10 hover:text-ink`}>
          <Minus size={14} />
        </button>
        <button aria-label="Maximizar" onClick={() => win.toggleMaximize()} className={`${btn} hover:bg-white/10 hover:text-ink`}>
          <Square size={11} />
        </button>
        <button aria-label="Cerrar" onClick={() => win.close()} className={`${btn} hover:bg-guinda hover:text-white`}>
          <X size={15} />
        </button>
      </div>
    </div>
  );
}
