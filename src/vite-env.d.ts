/// <reference types="vite/client" />

interface ImportMetaEnv {
  /** Base de la API de Control de Plazas, p. ej. https://servidor/api (ver lib/telemetry.ts) */
  readonly VITE_MONITOR_URL?: string;
  /** Llave compartida del monitoreo (OCR_MONITOR_KEY del backend) */
  readonly VITE_MONITOR_KEY?: string;
}
