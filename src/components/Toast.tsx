import { useCallback, useState } from "react";
import { AnimatePresence, motion } from "motion/react";
import { AlertCircle, AlertTriangle, CheckCircle2, X } from "lucide-react";
import type { Toast } from "../types";

let toastId = 0;

export function useToasts() {
  const [toasts, setToasts] = useState<Toast[]>([]);
  const remove = useCallback((id: number) => setToasts((t) => t.filter((x) => x.id !== id)), []);
  const add = useCallback(
    (t: Omit<Toast, "id">) => {
      const id = ++toastId;
      setToasts((prev) => [...prev, { ...t, id }]);
      window.setTimeout(() => remove(id), t.type === "error" ? 9000 : 5000);
    },
    [remove]
  );
  return { toasts, add, remove };
}

const STYLE = {
  success: { icon: CheckCircle2, cls: "border-mint/30 text-mint" },
  error: { icon: AlertCircle, cls: "border-rose/40 text-rose" },
  warning: { icon: AlertTriangle, cls: "border-warn/40 text-warn" },
};

export function ToastContainer({ toasts, onRemove }: { toasts: Toast[]; onRemove: (id: number) => void }) {
  return (
    <div className="pointer-events-none fixed bottom-5 right-5 z-40 flex w-[340px] flex-col gap-2" aria-live="polite">
      <AnimatePresence initial={false}>
        {toasts.map((t) => {
          const { icon: Icon, cls } = STYLE[t.type];
          return (
            <motion.div
              key={t.id}
              layout
              initial={{ opacity: 0, x: 40, scale: 0.96 }}
              animate={{ opacity: 1, x: 0, scale: 1 }}
              exit={{ opacity: 0, x: 40 }}
              transition={{ type: "spring", stiffness: 420, damping: 32 }}
              className={`pointer-events-auto flex items-start gap-3 rounded-xl border bg-panel/95 p-3.5 shadow-xl backdrop-blur ${cls}`}
            >
              <Icon size={18} className="mt-0.5 shrink-0" />
              <div className="flex min-w-0 flex-1 flex-col gap-0.5">
                <span className="text-[13px] font-medium text-ink">{t.title}</span>
                {t.message && <span className="text-xs leading-snug text-muted">{t.message}</span>}
              </div>
              <button aria-label="Cerrar aviso" onClick={() => onRemove(t.id)} className="text-faint hover:text-ink">
                <X size={14} />
              </button>
            </motion.div>
          );
        })}
      </AnimatePresence>
    </div>
  );
}
