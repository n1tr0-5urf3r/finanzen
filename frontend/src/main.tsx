import { StrictMode } from 'react';
import { createRoot } from 'react-dom/client';
import { BrowserRouter } from 'react-router-dom';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';

import App from './App';
import { AuthProvider } from './lib/auth';
import { I18nProvider } from './lib/i18n';
import { PrivacyProvider } from './lib/privacy';
import { ThemeProvider } from './lib/theme';
import './styles/index.css';

const queryClient = new QueryClient({
  defaultOptions: {
    queries: { staleTime: 30_000, retry: 1, refetchOnWindowFocus: false },
    mutations: { retry: false },
  },
});

createRoot(document.getElementById('root')!).render(
  <StrictMode>
    <QueryClientProvider client={queryClient}>
      <BrowserRouter>
        <I18nProvider>
          <ThemeProvider>
            <PrivacyProvider>
              <AuthProvider>
                <App />
              </AuthProvider>
            </PrivacyProvider>
          </ThemeProvider>
        </I18nProvider>
      </BrowserRouter>
    </QueryClientProvider>
  </StrictMode>,
);
