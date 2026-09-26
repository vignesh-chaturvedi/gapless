import { Route, Routes } from "react-router";

import { AppShell } from "@/components/shell/app-shell";
import { IncidentPage } from "@/routes/incident";
import { Incidents } from "@/routes/incidents";
import { Integrate } from "@/routes/integrate";
import { Landing } from "@/routes/landing";
import { Live } from "@/routes/live";
import { NotFound } from "@/routes/not-found";
import { Verify } from "@/routes/verify";

export function App() {
  return (
    <Routes>
      <Route index element={<Landing />} />
      <Route element={<AppShell />}>
        <Route path="live" element={<Live />} />
        <Route path="incidents" element={<Incidents />} />
        <Route path="incidents/:id" element={<IncidentPage />} />
        <Route path="verify" element={<Verify />} />
        <Route path="integrate" element={<Integrate />} />
        <Route path="*" element={<NotFound />} />
      </Route>
    </Routes>
  );
}
