import { Navigate, Route, Routes } from 'react-router-dom';

import { AppShell } from './components/AppShell';
import { ProtectedRoute } from './components/ProtectedRoute';
import { LoginPage, SetupPage } from './features/auth/AuthPages';
import { AnalysisPage } from './features/analysis/AnalysisPage';
import { CompareRedirect } from './features/compare/CompareRedirect';
import { BookingsPage } from './features/bookings/BookingsPage';
import { CategoriesPage } from './features/categories/CategoriesPage';
import { DashboardPage } from './features/dashboard/DashboardPage';
import { ImportPage } from './features/import/ImportPage';
import { KitchenOwlPage } from './features/kitchenowl/KitchenOwlPage';
import { MonthsPage } from './features/months/MonthsPage';
import { QuickAddPage } from './features/quickadd/QuickAddPage';
import { RecurringPage } from './features/recurring/RecurringPage';
import { FundsPage } from './features/funds/FundsPage';
import { SearchPage } from './features/search/SearchPage';
import { SettingsPage } from './features/settings/SettingsPage';
import { TaxPage } from './features/tax/TaxPage';

/**
 * German slugs, deliberately: they mirror the spreadsheet tabs the user already
 * navigates by name, and translating URLs would double the route table, break
 * bookmarks on a language switch, and drag data-ish vocabulary into the message
 * catalogue. API paths stay English.
 */
export default function App() {
  return (
    <Routes>
      <Route path="/einrichten" element={<SetupPage />} />
      <Route path="/anmelden" element={<LoginPage />} />
      <Route element={<ProtectedRoute />}>
        {/* Quick Add is full-bleed and lives outside the shell. */}
        <Route path="/schnell" element={<QuickAddPage />} />
        <Route element={<AppShell />}>
          <Route path="/dashboard" element={<DashboardPage />} />
          <Route path="/buchungen" element={<BookingsPage />} />
          <Route path="/suche" element={<SearchPage />} />
          <Route path="/monate" element={<MonthsPage />} />
          <Route path="/auswertung" element={<AnalysisPage />} />
          {/* Folded into /auswertung; the old address still works. */}
          <Route path="/vergleich" element={<CompareRedirect />} />
          <Route path="/kategorien" element={<CategoriesPage />} />
          <Route path="/import" element={<ImportPage />} />
          <Route path="/einstellungen" element={<SettingsPage />} />
          <Route path="/vorlagen" element={<RecurringPage />} />
          <Route path="/ruecklagen" element={<FundsPage />} />
          {/* A proper name, so the slug stays the proper name in both languages. */}
          <Route path="/kitchenowl" element={<KitchenOwlPage />} />
          <Route path="/steuer" element={<TaxPage />} />
        </Route>
      </Route>
      <Route path="/" element={<Navigate to="/dashboard" replace />} />
      <Route path="*" element={<Navigate to="/dashboard" replace />} />
    </Routes>
  );
}
