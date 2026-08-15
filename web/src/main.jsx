import { render } from 'preact';
import { useEffect, useState } from 'preact/hooks';
import './styles.css';
import { getToken, onTokenChange } from './lib/api.js';
import { ToastProvider } from './ui/Toast.jsx';
import { Login } from './views/Login.jsx';
import { Shell } from './views/Shell.jsx';

function App() {
  const [token, setToken] = useState(getToken());
  useEffect(() => onTokenChange(setToken), []);
  return token ? <Shell /> : <Login />;
}

render(
  <ToastProvider>
    <App />
  </ToastProvider>,
  document.getElementById('root'),
);
