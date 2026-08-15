import { useCallback, useEffect, useRef, useState } from 'preact/hooks';

/**
 * Carga asíncrona con estado de error y recarga manual.
 * Ignora respuestas que llegan después de desmontar o de una recarga posterior.
 */
export function useAsync(loader, dependencies = [], options = {}) {
  const { immediate = true } = options;
  const [state, setState] = useState({ data: null, error: null, loading: immediate });
  const generation = useRef(0);
  const mounted = useRef(true);

  useEffect(() => {
    mounted.current = true;
    return () => {
      mounted.current = false;
    };
  }, []);

  const run = useCallback(async () => {
    const current = ++generation.current;
    setState((previous) => ({ ...previous, loading: true, error: null }));
    try {
      const data = await loader();
      if (!mounted.current || current !== generation.current) return null;
      setState({ data, error: null, loading: false });
      return data;
    } catch (error) {
      if (!mounted.current || current !== generation.current) return null;
      setState({ data: null, error, loading: false });
      return null;
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, dependencies);

  useEffect(() => {
    if (immediate) run();
  }, [run, immediate]);

  return { ...state, reload: run };
}

/** Ejecuta `callback` cada `interval` ms mientras la pestaña esté visible. */
export function usePolling(callback, interval) {
  const saved = useRef(callback);
  saved.current = callback;

  useEffect(() => {
    if (!interval) return undefined;
    const tick = () => {
      if (document.visibilityState === 'visible') saved.current();
    };
    const handle = setInterval(tick, interval);
    document.addEventListener('visibilitychange', tick);
    return () => {
      clearInterval(handle);
      document.removeEventListener('visibilitychange', tick);
    };
  }, [interval]);
}

/** Retrasa el valor para no disparar una búsqueda por cada tecla. */
export function useDebounced(value, delay = 300) {
  const [debounced, setDebounced] = useState(value);
  useEffect(() => {
    const handle = setTimeout(() => setDebounced(value), delay);
    return () => clearTimeout(handle);
  }, [value, delay]);
  return debounced;
}

/** Ruta actual desde el hash: `#/containers` → `containers`. */
export function useRoute() {
  const read = () => (window.location.hash.replace(/^#\/?/, '').split('?')[0] || 'dashboard');
  const [route, setRoute] = useState(read);

  useEffect(() => {
    const onChange = () => setRoute(read());
    window.addEventListener('hashchange', onChange);
    return () => window.removeEventListener('hashchange', onChange);
  }, []);

  return route;
}

export function navigate(route) {
  window.location.hash = `#/${route}`;
}

/** Cierra con Escape y devuelve el foco al abrir un diálogo. */
export function useEscape(onEscape, active = true) {
  useEffect(() => {
    if (!active) return undefined;
    const onKeyDown = (event) => {
      if (event.key === 'Escape') {
        event.stopPropagation();
        onEscape();
      }
    };
    window.addEventListener('keydown', onKeyDown);
    return () => window.removeEventListener('keydown', onKeyDown);
  }, [onEscape, active]);
}
