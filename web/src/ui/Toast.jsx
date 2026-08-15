import { createContext } from 'preact';
import { useCallback, useContext, useMemo, useState } from 'preact/hooks';
import { AlertTriangle, CheckCircle2, Info, X } from 'lucide-preact';

const ToastContext = createContext(() => {});
const ICONS = { success: CheckCircle2, error: AlertTriangle, info: Info };

let nextId = 1;

export function ToastProvider({ children }) {
  const [toasts, setToasts] = useState([]);

  const dismiss = useCallback((id) => {
    setToasts((current) => current.filter((toast) => toast.id !== id));
  }, []);

  const push = useCallback(
    (message, tone = 'info', timeout = tone === 'error' ? 8000 : 4500) => {
      const id = nextId++;
      setToasts((current) => [...current.slice(-3), { id, message, tone }]);
      if (timeout) setTimeout(() => dismiss(id), timeout);
      return id;
    },
    [dismiss],
  );

  const toast = useMemo(
    () => ({
      info: (message) => push(message, 'info'),
      success: (message) => push(message, 'success'),
      error: (message) => push(message?.message || message, 'error'),
    }),
    [push],
  );

  return (
    <ToastContext.Provider value={toast}>
      {children}
      <div class="toast-region" aria-live="polite" aria-atomic="true">
        {toasts.map((item) => {
          const Icon = ICONS[item.tone] || Info;
          return (
            <div key={item.id} class={`toast toast-${item.tone}`}>
              <Icon size={16} />
              <p>{item.message}</p>
              <button type="button" onClick={() => dismiss(item.id)} aria-label="Descartar">
                <X size={14} />
              </button>
            </div>
          );
        })}
      </div>
    </ToastContext.Provider>
  );
}

export function useToast() {
  return useContext(ToastContext);
}
