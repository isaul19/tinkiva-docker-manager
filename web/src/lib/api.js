// Cliente HTTP del panel. Solo la sesión opaca vive en sessionStorage; las
// contraseñas nunca se guardan en el navegador.

const TOKEN_KEY = 'tdm-token';
const MUST_CHANGE_KEY = 'tdm-must-change-password';
const BASE_PATH = window.location.pathname.replace(/[^/]*$/, '');

let token = sessionStorage.getItem(TOKEN_KEY) || '';
let mustChangePassword = sessionStorage.getItem(MUST_CHANGE_KEY) === 'true';
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

export function getAuthState() {
  return { token, mustChangePassword };
}

export function setToken(value, mustChange = false) {
  token = value;
  mustChangePassword = mustChange;
  sessionStorage.setItem(TOKEN_KEY, value);
  sessionStorage.setItem(MUST_CHANGE_KEY, String(mustChange));
  listeners.forEach((listener) => listener(getAuthState()));
}

export function clearToken() {
  token = '';
  mustChangePassword = false;
  sessionStorage.removeItem(TOKEN_KEY);
  sessionStorage.removeItem(MUST_CHANGE_KEY);
  listeners.forEach((listener) => listener(getAuthState()));
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
  if (token && !options.skipAuth) headers.Authorization = `Bearer ${token}`;
  if (options.token) headers.Authorization = `Bearer ${options.token}`;

  let body;
  if (options.form) {
    headers['Content-Type'] = 'application/x-www-form-urlencoded';
    body = encodeForm(options.form);
  }
  // El archivo viaja como cuerpo crudo: el navegador lo transmite en streaming
  // y el panel lo escribe en disco según llega, sin pasar por memoria.
  if (options.file) {
    headers['Content-Type'] = 'application/sql';
    body = options.file;
  }

  let response;
  try {
    const url = `${BASE_PATH}${path.replace(/^\//, '')}`;
    response = await fetch(url, { method, headers, body });
  } catch (cause) {
    throw new ApiError('No se pudo contactar con el panel', 0, cause);
  }

  // Un 401 significa que el token dejó de servir: cerramos sesión de inmediato.
  if (response.status === 401 && !options.token && !options.skipAuth) {
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

  /** POST de un archivo del disco del usuario como cuerpo de la petición. */
  upload: (path, file, query) => request('POST', `${path}${buildQuery(query)}`, { file }),

  async signIn(username, password) {
    const session = await request('POST', '/api/auth/login', {
      form: { username, password },
      skipAuth: true,
    });
    setToken(session.token, session.must_change_password);
    return session;
  },

  async changePassword(password) {
    const session = await request('POST', '/api/auth/change-password', { form: { password } });
    setToken(session.token, false);
    return session;
  },
};
