import { invoke } from "@tauri-apps/api/core";
import { getVersion } from "@tauri-apps/api/app";
import { listen } from "@tauri-apps/api/event";
import type { Progress, RedactProgress } from "../types";

/**
 * Reporte de uso a Control de Plazas (widget "Monitoreo OCR").
 *
 * Nunca bloquea al usuario: sin red, sin URL configurada o con el servidor
 * caído, el programa funciona igual y los eventos esperan en una cola (en
 * memoria y en localStorage) hasta que haya conexión. No se envía el texto de
 * las búsquedas ni el contenido o el nombre de los documentos: solo cuántos.
 *
 * Configuración al compilar (archivo .env, ver .env.example):
 *   VITE_MONITOR_URL  base de la API, p. ej. https://servidor/api
 *   VITE_MONITOR_KEY  llave compartida (OCR_MONITOR_KEY del backend)
 */

const URL_BASE = (import.meta.env.VITE_MONITOR_URL as string | undefined)?.replace(/\/+$/, "") ?? "";
const LLAVE = (import.meta.env.VITE_MONITOR_KEY as string | undefined) ?? "";
const ACTIVO = !!URL_BASE && !!LLAVE;

const LATIDO_MS = 60_000;
const MAX_COLA = 1000;
const CLAVE_COLA = "monitoreo.cola";
const CLAVE_USUARIO = "monitoreo.usuario";

type Tipo = "carga_carpeta" | "busqueda" | "ver_documento" | "abrir_documento" | "testado_analisis" | "testado_guardado" | "error";
interface Evento {
  tipo: Tipo;
  cantidad: number;
  detalle?: Record<string, string | number | boolean | null>;
  fecha: string;
}
export interface Usuario {
  nombre: string;
  numEmpleado: string;
}

let usuario: Usuario | null = null;
let sesion: { id: number; token: string } | null = null;
let inicio = new Date().toISOString();
let cola: Evento[] = leerCola();
/** Acciones frecuentes (búsquedas, vistas) se agrupan y se mandan como un solo evento por envío. */
const contadores = new Map<Tipo, number>();
let enviando = false;
let latido: number | undefined;
/** Qué tarea de testado está corriendo: el evento de fin no dice si fue análisis o guardado. */
let testadoEnCurso: "analisis" | "guardado" | null = null;

function leerCola(): Evento[] {
  try {
    const guardada = JSON.parse(localStorage.getItem(CLAVE_COLA) || "[]");
    return Array.isArray(guardada) ? guardada : [];
  } catch {
    return [];
  }
}

function guardarCola() {
  try {
    localStorage.setItem(CLAVE_COLA, JSON.stringify(cola.slice(-MAX_COLA)));
  } catch {
    /* sin almacenamiento: la cola queda solo en memoria */
  }
}

function encolar(tipo: Tipo, cantidad = 1, detalle?: Evento["detalle"]) {
  if (!ACTIVO || !usuario) return;
  cola.push({ tipo, cantidad, detalle, fecha: new Date().toISOString() });
  if (cola.length > MAX_COLA) cola = cola.slice(-MAX_COLA);
  guardarCola();
}

async function post(ruta: string, cuerpo: unknown): Promise<Response> {
  return fetch(`${URL_BASE}${ruta}`, {
    method: "POST",
    headers: { "Content-Type": "application/json", "X-Monitor-Key": LLAVE },
    body: JSON.stringify(cuerpo),
  });
}

async function abrirSesion(): Promise<boolean> {
  if (!usuario) return false;
  const [version, equipo] = await Promise.all([getVersion().catch(() => ""), invoke<string>("equipo").catch(() => "")]);
  const res = await post("/monitoreo-ocr/ingesta/sesion/", { nombre: usuario.nombre, num_empleado: usuario.numEmpleado, version, equipo, inicio });
  if (!res.ok) return false;
  const data = await res.json();
  sesion = { id: data.sesion_id, token: data.token };
  return true;
}

/** Manda la cola y los contadores; sin eventos, sirve de latido. */
async function enviar(cerrar = false): Promise<void> {
  if (!ACTIVO || !usuario || enviando) return;
  enviando = true;
  try {
    for (const [tipo, n] of contadores) if (n > 0) encolar(tipo, n);
    contadores.clear();
    if (!sesion && !(await abrirSesion())) return;
    const lote = cola.slice(0, 200);
    let res = await post("/monitoreo-ocr/ingesta/eventos/", { sesion_id: sesion!.id, token: sesion!.token, eventos: lote, cerrar });
    if (res.status === 401) {
      // El servidor ya no reconoce la sesión (p. ej. se reinició la base): se abre otra.
      sesion = null;
      if (!(await abrirSesion())) return;
      res = await post("/monitoreo-ocr/ingesta/eventos/", { sesion_id: sesion!.id, token: sesion!.token, eventos: lote, cerrar });
    }
    if (res.ok) {
      cola = cola.slice(lote.length);
      guardarCola();
    }
  } catch {
    /* sin conexión: se reintenta en el siguiente latido */
  } finally {
    enviando = false;
  }
}

export const telemetry = {
  activo: ACTIVO,

  usuarioGuardado(): Usuario | null {
    try {
      return JSON.parse(localStorage.getItem(CLAVE_USUARIO) || "null");
    } catch {
      return null;
    }
  },

  /** Identifica al usuario y arranca el envío periódico. */
  iniciar(u: Usuario) {
    usuario = u;
    inicio = new Date().toISOString();
    sesion = null;
    try {
      localStorage.setItem(CLAVE_USUARIO, JSON.stringify(u));
    } catch {
      /* ignorado */
    }
    if (!ACTIVO) return;
    void enviar();
    window.clearInterval(latido);
    latido = window.setInterval(() => void enviar(), LATIDO_MS);
  },

  /** Último envío antes de cerrar el programa; no espera más de 2 s. */
  async cerrar() {
    window.clearInterval(latido);
    await Promise.race([enviar(true), new Promise((r) => setTimeout(r, 2000))]);
  },

  contar(tipo: "busqueda" | "ver_documento" | "abrir_documento") {
    contadores.set(tipo, (contadores.get(tipo) ?? 0) + 1);
  },

  error(titulo: string, mensaje: string) {
    encolar("error", 1, { titulo: titulo.slice(0, 200), mensaje: mensaje.slice(0, 500) });
  },

  testado(tarea: "analisis" | "guardado") {
    testadoEnCurso = tarea;
  },
};

// Resultados de las tareas largas: se leen de los mismos eventos que ya usa la interfaz.
void listen<Progress>("index-progress", ({ payload: p }) => {
  if (p.phase !== "done" && p.phase !== "cancelled") return;
  const procesados = p.done - p.errors;
  if (procesados <= 0 && p.errors === 0) return; // carga sin documentos nuevos
  encolar("carga_carpeta", Math.max(procesados, 0), {
    total_carpeta: p.total,
    sin_cambios: p.unchanged,
    errores: p.errors,
    ocr_documentos: p.ocr_files ?? 0,
    ocr_paginas: p.ocr_pages,
    segundos: Math.round(p.elapsed_ms / 1000),
    detenida: p.phase === "cancelled",
  });
});

void listen<RedactProgress>("redact-progress", ({ payload: p }) => {
  if (p.phase === "analyzing" || p.phase === "saving" || p.phase === "error") return;
  const tarea = testadoEnCurso;
  testadoEnCurso = null;
  if (tarea === "analisis") {
    encolar("testado_analisis", p.done_files, { paginas: p.done_pages, segundos: Math.round(p.elapsed_ms / 1000), detenido: p.phase === "cancelled" });
  } else if (tarea === "guardado") {
    const archivos = p.report?.files ?? [];
    const ok = archivos.filter((f) => !f.error);
    encolar("testado_guardado", ok.length, {
      paginas: ok.reduce((s, f) => s + f.pages, 0),
      errores: archivos.length - ok.length,
      segundos: Math.round(p.elapsed_ms / 1000),
      detenido: p.phase === "cancelled",
    });
  }
});

// Errores no controlados de la interfaz
window.addEventListener("error", (e) => telemetry.error("Error de la interfaz", String(e.message || e.error)));
window.addEventListener("unhandledrejection", (e) => telemetry.error("Error de la interfaz", String(e.reason)));
