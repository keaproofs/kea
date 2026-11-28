import { useEffect } from "react";
import { isAddress, report as readReport } from "./api";
import { BurnPage } from "./routes/Burn";
import { HomeV2 } from "./routes/HomeV2";
import { ReportPage } from "./routes/Report";
import { usePath } from "./router";

/** Starts loading a report when a link to it is pointed at or focused, before the click. */
function useWarmReports() {
  useEffect(() => {
    const warm = (event: Event) => {
      const link = event.target instanceof Element ? event.target.closest('a[href^="/c/0x"]') : null;
      const address = link?.getAttribute("href")?.slice(3);
      if (address && isAddress(address)) readReport(address).catch(() => undefined);
    };
    document.addEventListener("pointerover", warm, { passive: true });
    document.addEventListener("focusin", warm);
    return () => {
      document.removeEventListener("pointerover", warm);
      document.removeEventListener("focusin", warm);
    };
  }, []);
}

export function App() {
  useWarmReports();
  const path = usePath();
  if (/^\/burn\/?$/.test(path)) return <BurnPage />;
  const report = path.match(/^\/c\/(0x[0-9a-fA-F]{40})\/?$/);
  return report ? <ReportPage address={report[1].toLowerCase()} /> : <HomeV2 />;
}
