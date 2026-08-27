import { createContext } from 'preact';
import { useContext } from 'preact/hooks';

/**
 * Datos globales de observabilidad y una señal para refrescar las vistas.
 */
export const AppContext = createContext(null);

export function useApp() {
  return useContext(AppContext);
}
