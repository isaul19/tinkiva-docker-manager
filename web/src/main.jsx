import { render } from 'preact';
import { useEffect, useState } from 'preact/hooks';
import './styles.css';
import { getAuthState, onTokenChange } from './lib/api.js';
import { ToastProvider } from './ui/Toast.jsx';
import { Login } from './views/Login.jsx';
import { Shell } from './views/Shell.jsx';

function App() {
  const [auth, setAuth] = useState(getAuthState());
  useEffect(() => onTokenChange(setAuth), []);
  if (!auth.token || auth.mustChangePassword) {
    return <Login mustChangePassword={auth.mustChangePassword} />;
  }
  return <Shell />;
}

render(
  <ToastProvider>
    <App />
  </ToastProvider>,
  document.getElementById('root'),
);
