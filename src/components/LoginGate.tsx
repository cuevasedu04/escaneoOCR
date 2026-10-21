import { useState, type FormEvent, type ReactNode } from "react";
import { LogIn } from "lucide-react";
import { TitleBar } from "./TitleBar";
import { GlowCard } from "./ui/GlowCard";
import { Button } from "./ui/Button";
import { telemetry, type Usuario } from "../lib/telemetry";

/**
 * Identificación al abrir el programa: solo nombre y número de empleado, para
 * saber quién lo usa (monitoreo en Control de Plazas). No es una contraseña;
 * se recuerdan los últimos datos para que baste con presionar Entrar.
 */
export function LoginGate({ children }: { children: ReactNode }) {
  const guardado = telemetry.usuarioGuardado();
  const [usuario, setUsuario] = useState<Usuario | null>(null);
  const [nombre, setNombre] = useState(guardado?.nombre ?? "");
  const [num, setNum] = useState(guardado?.numEmpleado ?? "");

  if (usuario) return <>{children}</>;

  const nombreOk = nombre.trim().length >= 3;
  const numOk = /^\d{4,11}$/.test(num.trim());

  const entrar = (e: FormEvent) => {
    e.preventDefault();
    if (!nombreOk || !numOk) return;
    const u = { nombre: nombre.trim().replace(/\s+/g, " "), numEmpleado: num.trim() };
    telemetry.iniciar(u);
    setUsuario(u);
  };

  const input =
    "h-11 w-full rounded-xl border border-white/[0.14] bg-white/[0.04] px-3.5 text-[14px] text-ink placeholder:text-faint focus:border-gold/70 focus:outline-none";

  return (
    <div className="flex h-screen flex-col overflow-hidden bg-ground">
      <TitleBar badge={null} />
      <main className="flex flex-1 items-center justify-center px-6">
        <GlowCard interactive={false} className="w-full max-w-[400px] p-7">
          <form onSubmit={entrar} className="flex flex-col gap-4">
            <div>
              <h1 className="m-0 text-lg font-semibold text-ink">Búsqueda de Documentos</h1>
              <p className="mt-1.5 text-xs leading-relaxed text-muted">Escribe tu nombre y tu número de empleado para identificar quién usa el programa.</p>
            </div>
            <label className="flex flex-col gap-1.5 text-xs text-ink2">
              Nombre completo
              <input autoFocus={!guardado} value={nombre} onChange={(e) => setNombre(e.target.value)} maxLength={200} spellCheck={false} className={input} />
            </label>
            <label className="flex flex-col gap-1.5 text-xs text-ink2">
              Número de empleado
              <input
                value={num}
                onChange={(e) => setNum(e.target.value.replace(/\D/g, ""))}
                inputMode="numeric"
                maxLength={11}
                className={`${input} font-mono`}
              />
              {num && !numOk && <span className="text-[11px] text-warn">Solo números, de 4 a 11 dígitos.</span>}
            </label>
            <Button type="submit" variant="primary" size="lg" icon={<LogIn size={15} />} disabled={!nombreOk || !numOk} autoFocus={!!guardado}>
              Entrar
            </Button>
          </form>
        </GlowCard>
      </main>
    </div>
  );
}
