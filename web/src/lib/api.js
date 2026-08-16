// Cliente HTTP del panel. El token vive solo en sessionStorage: se pierde al
// cerrar la pestaña y nunca se escribe en localStorage ni en cookies.

const TOKEN_KEY = 'tdm-token';

let token = sessionStorage.getItem(TOKEN_KEY) || '';
const listeners = new Set();

export class ApiError extends Error {
  constructor(message, status, payload) {
    super(message);
    this.name = 'ApiError';
    this.status = status;
    this.payload = payload;
  }
}

export function getToken() {
  return token;
}

export function setToken(value) {
  token = value;
  sessionStorage.setItem(TOKEN_KEY, value);
  listeners.forEach((listener) => listener(token));
}

export function clearToken() {
  token = '';
  sessionStorage.removeItem(TOKEN_KEY);
  listeners.forEach((listener) => listener(token));
}

export function onTokenChange(listener) {
  listeners.add(listener);
  return () => listeners.delete(listener);
}

function encodeForm(fields) {
  return Object.entries(fields)
    .filter(([, value]) => value !== undefined && value !== null && value !== '')
    .map(([key, value]) => `${encodeURIComponent(key)}=${encodeURIComponent(value)}`)
    .join('&');
}

function buildQuery(query) {
  const encoded = encodeForm(query || {});
  return encoded ? `?${encoded}` : '';
}

async function request(method, path, options = {}) {
  const headers = {};
  if (token) headers.Authorization = `Bearer ${token}`;
  if (options.token) headers.Authorization = `Bearer ${options.token}`;

  let body;
  if (options.form) {
    headers['Content-Type'] = 'application/x-www-form-urlencoded';
    body = encodeForm(options.form);
  }

  let response;
  try {
    response = await fetch(path, { method, headers, body });
  } catch (cause) {
    throw new ApiError('No se pudo contactar con el panel', 0, cause);
  }

  // Un 401 significa que el token dejó de servir: cerramos sesión de inmediato.
  if (response.status === 401 && !options.token) {
    clearToken();
    throw new ApiError('La sesión ya no es válida', 401);
  }

  // Las descargas pueden pesar cientos de MB: se devuelven como Blob para no
  // materializarlas también como string.
  if (options.asBlob) {
    if (response.ok) return response.blob();
    const detail = await response.text();
    let payload = null;
    try {
      payload = JSON.parse(detail);
    } catch {
      payload = null;
    }
    throw new ApiError((payload && payload.error) || detail || `Error ${response.status}`, response.status, payload);
  }

  const text = await response.text();
  if (options.asText) {
    if (!response.ok) throw new ApiError(text || `Error ${response.status}`, response.status);
    return text;
  }

  let data = null;
  if (text) {
    try {
      data = JSON.parse(text);
    } catch {
      data = null;
    }
  }
  if (!response.ok) {
    throw new ApiError(
      (data && data.error) || text || `Error ${response.status}`,
      response.status,
      data,
    );
  }
  return data;
}

export const api = {
  get: (path, query) => request('GET', `${path}${buildQuery(query)}`),
  text: (path, query) => request('GET', `${path}${buildQuery(query)}`, { asText: true }),
  post: (path, form) => request('POST', path, { form: form || {} }),
  del: (path, query) => request('DELETE', `${path}${buildQuery(query)}`),

  /** POST que devuelve un archivo. El nombre lo pone quien llama. */
  download: (path, form) => request('POST', path, { form: form || {}, asBlob: true }),

  /** Valida un token contra /api/info antes de guardarlo. */
  async signIn(candidate) {
    const info = await request('GET', '/api/info', { token: candidate });
    setToken(candidate);
    return info;
  },
};
