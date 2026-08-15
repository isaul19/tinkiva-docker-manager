import { createContext } from 'preact';
import { useContext } from 'preact/hooks';

/**
 * Datos globales del panel: `/api/info`, `/api/catalog` y utilidades para
 * refrescarlos. Evita que cada vista vuelva a pedir el catálogo de recursos.
 */
export const AppContext = createContext(null);

export function useApp() {
  return useContext(AppContext);
}
