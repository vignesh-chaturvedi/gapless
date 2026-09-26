import { lazy, Suspense } from "react";
import { Route, Routes } from "react-router";

import { AppShell } from "@/components/shell/app-shell";
import { Skeleton } from "@/components/ui/skeleton";
import { Landing } from "@/routes/landing";
import { NotFound } from "@/routes/not-found";

// The landing page loads first; console pages load on the way in.
const Live = lazy(() => import("@/routes/live").then((m) => ({ default: m.Live })));
const Incidents = lazy(() => import("@/routes/incidents").then((m) => ({ default: m.Incidents })));
const IncidentPage = lazy(() => import("@/routes/incident").then((m) => ({ default: m.IncidentPage })));
const Verify = lazy(() => import("@/routes/verify").then((m) => ({ default: m.Verify })));
const Integrate = lazy(() => import("@/routes/integrate").then((m) => ({ default: m.Integrate })));

function PageLoading() {
  return (
    <div className="mx-auto flex max-w-[1400px] flex-col gap-4 px-4 py-6 lg:px-6" aria-busy="true" aria-label="Loading">
      <Skeleton className="h-7 w-40" />
      <Skeleton className="h-52 rounded-lg" />
      <Skeleton className="h-24 rounded-lg" />
    </div>
  );
}

const page = (element: React.ReactNode) => <Suspense fallback={<PageLoading />}>{element}</Suspense>;

export function App() {
  return (
    <Routes>
      <Route index element={<Landing />} />
      <Route element={<AppShell />}>
        <Route path="live" element={page(<Live />)} />
        <Route path="incidents" element={page(<Incidents />)} />
        <Route path="incidents/:id" element={page(<IncidentPage />)} />
        <Route path="verify" element={page(<Verify />)} />
        <Route path="integrate" element={page(<Integrate />)} />
        <Route path="*" element={<NotFound />} />
      </Route>
    </Routes>
  );
}
