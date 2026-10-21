import { useRef, type CSSProperties, type MouseEvent, type ReactNode } from "react";

interface GlowCardProps {
  children: ReactNode;
  className?: string;
  glow?: string;
  line?: string;
  interactive?: boolean;
}

/** Tarjeta con resplandor que sigue al cursor y línea de luz superior. */
export function GlowCard({
  children,
  className = "",
  glow = "rgba(188,149,92,0.10)",
  line = "rgba(188,149,92,0.7)",
  interactive = true,
}: GlowCardProps) {
  const ref = useRef<HTMLDivElement>(null);

  function onMove(e: MouseEvent) {
    const el = ref.current;
    if (!el) return;
    const r = el.getBoundingClientRect();
    el.style.setProperty("--x", `${e.clientX - r.left}px`);
    el.style.setProperty("--y", `${e.clientY - r.top}px`);
  }

  return (
    <div
      ref={ref}
      onMouseMove={interactive ? onMove : undefined}
      style={{ "--glow": glow } as CSSProperties}
      className={`group relative overflow-hidden rounded-2xl border border-white/[0.08] bg-panel ${
        interactive ? "transition-[border-color,transform] duration-300 hover:border-gold/40" : ""
      } ${className}`}
    >
      {interactive && (
        <div
          aria-hidden
          className="pointer-events-none absolute inset-0 opacity-0 transition-opacity duration-300 group-hover:opacity-100"
          style={{ background: "radial-gradient(320px circle at var(--x, 50%) var(--y, 0%), var(--glow), transparent 70%)" }}
        />
      )}
      <div
        aria-hidden
        className="pointer-events-none absolute left-6 right-6 top-0 h-px"
        style={{ background: `linear-gradient(90deg, transparent, ${line}, transparent)` }}
      />
      <div className="relative h-full">{children}</div>
    </div>
  );
}
