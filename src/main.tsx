import {StrictMode} from 'react';
import {createRoot} from 'react-dom/client';
import App from './App.tsx';
import './index.css';
import { openExternal } from './lib/tauri';
import { I18nProvider } from './i18n';

// The app window never navigates away: web and email links open in the default app.
document.addEventListener('click', (event) => {
  const link = (event.target as HTMLElement | null)?.closest?.('a[href]') as HTMLAnchorElement | null;
  if (!link) return;
  const href = link.getAttribute('href') ?? '';
  if (/^(https?:|mailto:)/i.test(href)) {
    event.preventDefault();
    openExternal(href).catch((err) => console.error('Could not open link:', err));
  }
});

createRoot(document.getElementById('root')!).render(
  <StrictMode>
    <I18nProvider>
      <App />
    </I18nProvider>
  </StrictMode>,
);
