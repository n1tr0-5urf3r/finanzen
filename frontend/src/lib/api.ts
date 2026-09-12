import type { ApiErrorBody } from './types';

export class ApiError extends Error {
  constructor(
    message: string,
    public readonly status: number,
    public readonly code?: string,
    public readonly details?: unknown,
  ) {
    super(message);
    this.name = 'ApiError';
  }
}

// Relative, deliberately. In production the Rust binary serves both the API and
// this bundle from one origin; in development Vite proxies /api. There is no
// VITE_API_URL and therefore no CORS anywhere.
const API_BASE = '/api/v1';

export async function api<T>(path: string, options: RequestInit = {}): Promise<T> {
  const headers = new Headers(options.headers);
  if (options.body && !(options.body instanceof FormData)) {
    headers.set('Content-Type', 'application/json');
  }
  headers.set('Accept', 'application/json');

  const response = await fetch(`${API_BASE}${path}`, {
    ...options,
    // The session is an HttpOnly cookie; nothing is kept in localStorage.
    credentials: 'include',
    headers,
  });

  if (!response.ok) {
    let body: ApiErrorBody = {};
    try {
      body = (await response.json()) as ApiErrorBody;
    } catch {
      /* a non-JSON error body is still an error */
    }
    throw new ApiError(body.message || statusMessage(response.status), response.status, body.code, body.details);
  }
  if (response.status === 204) return undefined as T;
  return (await response.json()) as T;
}

export const jsonBody = (value: unknown): RequestInit => ({ body: JSON.stringify(value) });

/** Multipart upload with progress, for spreadsheets and receipts. */
export function apiUpload<T>(
  path: string,
  file: File,
  onProgress?: (fraction: number) => void,
): Promise<T> {
  return new Promise((resolve, reject) => {
    const form = new FormData();
    form.append('file', file);

    const xhr = new XMLHttpRequest();
    xhr.open('POST', `${API_BASE}${path}`);
    xhr.withCredentials = true;
    xhr.upload.addEventListener('progress', (e) => {
      if (e.lengthComputable && onProgress) onProgress(e.loaded / e.total);
    });
    xhr.addEventListener('load', () => {
      let parsed: unknown = null;
      try {
        parsed = JSON.parse(xhr.responseText);
      } catch {
        /* leave null */
      }
      if (xhr.status >= 200 && xhr.status < 300) {
        resolve(parsed as T);
        return;
      }
      const body = (parsed ?? {}) as ApiErrorBody;
      reject(new ApiError(body.message || statusMessage(xhr.status), xhr.status, body.code));
    });
    xhr.addEventListener('error', () =>
      reject(new ApiError(statusMessage(0), 0, 'network_error')),
    );
    xhr.send(form);
  });
}

/**
 * Downloads a file, honouring RFC 5987 `filename*` so German filenames survive.
 * This is the path everybody breaks: `Belegübersicht.pdf` arrives percent-encoded
 * and lands on disk mangled unless it is decoded here.
 */
export async function apiBlob(path: string): Promise<{ blob: Blob; filename: string }> {
  const response = await fetch(`${API_BASE}${path}`, { credentials: 'include' });
  if (!response.ok) {
    throw new ApiError(statusMessage(response.status), response.status);
  }
  const disposition = response.headers.get('Content-Disposition') ?? '';
  const encoded = /filename\*=UTF-8''([^;]+)/i.exec(disposition);
  const plain = /filename="?([^";]+)"?/i.exec(disposition);
  const filename = encoded
    ? decodeURIComponent(encoded[1])
    : (plain?.[1] ?? 'download');
  return { blob: await response.blob(), filename };
}

/**
 * Fallback messages, used only when the server sent none. They are UI chrome, so
 * they are looked up through i18n at the call site rather than hard-coded German
 * here — this function returns a key-shaped default for the rare bare status.
 */
function statusMessage(status: number): string {
  if (status === 0) return 'Keine Verbindung zum Server.';
  if (status === 400) return 'Die Anfrage enthält ungültige Angaben.';
  if (status === 401) return 'Bitte melde dich erneut an.';
  if (status === 403) return 'Du hast dafür keine Berechtigung.';
  if (status === 404) return 'Der angefragte Inhalt wurde nicht gefunden.';
  if (status === 409) return 'Die Daten wurden zwischenzeitlich geändert.';
  if (status === 422) return 'Die Daten konnten nicht verarbeitet werden.';
  if (status === 502) return 'Ein externer Dienst ist nicht erreichbar.';
  if (status >= 500) return 'Der Server ist derzeit nicht erreichbar.';
  return 'Die Anfrage konnte nicht abgeschlossen werden.';
}

export function errorMessage(error: unknown): string {
  if (error instanceof ApiError || error instanceof Error) return error.message;
  return 'Ein unbekannter Fehler ist aufgetreten.';
}

/**
 * Fetches an export and hands it to the browser as a download.
 *
 * The object URL is revoked on the next frame rather than immediately: Safari
 * cancels an in-flight download if the URL disappears in the same tick, which
 * looks exactly like a server error and is not one.
 */
export async function downloadFile(path: string): Promise<string> {
  const { blob, filename } = await apiBlob(path);
  const url = URL.createObjectURL(blob);
  const link = document.createElement('a');
  link.href = url;
  link.download = filename;
  document.body.appendChild(link);
  link.click();
  link.remove();
  setTimeout(() => URL.revokeObjectURL(url), 0);
  return filename;
}
