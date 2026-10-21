import type { ButtonHTMLAttributes, ReactNode } from "react";

type Variant = "primary" | "danger" | "gold" | "ghost" | "quiet";

const VARIANTS: Record<Variant, string> = {
  primary: "bg-jade text-[#F3FBF7] hover:bg-[#2B9774] border border-transparent",
  danger: "bg-guinda text-[#FFF4F6] hover:bg-[#B3284B] border border-transparent",
  gold: "bg-gold/10 text-goldhi border border-gold/40 hover:bg-gold/20",
  ghost: "bg-white/[0.04] text-ink border border-white/[0.14] hover:bg-white/[0.08] hover:border-white/25",
  quiet: "bg-transparent text-muted border border-white/[0.12] hover:text-ink hover:border-white/25",
};

interface ButtonProps extends ButtonHTMLAttributes<HTMLButtonElement> {
  variant?: Variant;
  size?: "sm" | "md" | "lg";
  icon?: ReactNode;
}

export function Button({ variant = "ghost", size = "md", icon, className = "", children, ...rest }: ButtonProps) {
  const h = size === "sm" ? "h-8 px-3 text-xs rounded-lg" : size === "lg" ? "h-11 px-4 text-[13px] rounded-xl" : "h-10 px-4 text-[13px] rounded-xl";
  return (
    <button
      {...rest}
      className={`inline-flex items-center justify-center gap-2 font-medium transition-all duration-200 active:scale-[0.98] disabled:cursor-not-allowed disabled:opacity-40 disabled:active:scale-100 ${h} ${VARIANTS[variant]} ${className}`}
    >
      {icon}
      {children}
    </button>
  );
}
