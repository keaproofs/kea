import { useMemo, useState } from "react";
import { checkedOn, short, type Listed } from "../api";
import { follow } from "../router";

const TABS = [
  { id: "all", label: "All", keep: () => true },
  { id: "found", label: "Powers found", keep: (r: Listed) => r.summary.found > 0 },
  { id: "proven", label: "Fully proven", keep: (r: Listed) => r.summary.found === 0 && r.summary.unclear === 0 },
  { id: "unclear", label: "Left unclear", keep: (r: Listed) => r.summary.unclear > 0 },
] as const;

/** Every report, filterable, dense like a trading board: one row per contract. */
export function ReportGrid({ reports }: { reports: Listed[] }) {
  const [tab, setTab] = useState<(typeof TABS)[number]["id"]>("all");
  const rows = useMemo(() => reports.filter(TABS.find((t) => t.id === tab)!.keep), [reports, tab]);
  return (
    <div className="kx-grid">
      <div className="kx-tabs" role="tablist" aria-label="Filter reports">
        {TABS.map((t) => {
          const count = reports.filter(t.keep).length;
          return (
            <button
              key={t.id}
              role="tab"
              aria-selected={tab === t.id}
              className="kx-tab"
              onClick={() => setTab(t.id)}
              type="button"
            >
              {t.label}
              <span className="kx-tab-count">{count}</span>
            </button>
          );
        })}
      </div>
      <div className="kx-table" role="table" aria-label="Reports">
        <div className="kx-row kx-row-head" role="row">
          <span role="columnheader">Contract</span>
          <span role="columnheader">Proven</span>
          <span role="columnheader">Found</span>
          <span role="columnheader">Unclear</span>
          <span role="columnheader">Checked</span>
        </div>
        {rows.map((report, i) => (
          <a
            key={`${tab}-${report.address}`}
            className="kx-row"
            role="row"
            href={`/c/${report.address}`}
            onClick={follow}
            style={{ ["--i" as string]: i }}
          >
            <span role="cell" className="kx-row-name">
              <strong>{report.name}</strong>
              <span className="mono">{short(report.address)}</span>
            </span>
            <span role="cell" className={`kx-num ${report.summary.proven ? "kx-word-proven" : "kx-zero"}`}>
              {report.summary.proven} proven
            </span>
            <span role="cell" className={`kx-num ${report.summary.found ? "kx-word-found" : "kx-zero"}`}>
              {report.summary.found} found
            </span>
            <span role="cell" className={`kx-num ${report.summary.unclear ? "kx-word-unclear" : "kx-zero"}`}>
              {report.summary.unclear} unclear
            </span>
            <span role="cell" className="kx-row-time">
              {report.stale ? (
                <span className="kx-word-found">Code changed, re-checking</span>
              ) : (
                <When seconds={report.checked_at} />
              )}
            </span>
          </a>
        ))}
        {rows.length === 0 && <p className="kx-empty">No contract here yet.</p>}
      </div>
    </div>
  );
}

/** "25 Nov 2026" always, and ", 12:21 UTC" where the column has room for it. */
function When({ seconds }: { seconds: number }) {
  const [day, time] = checkedOn(seconds).split(", ");
  return (
    <>
      {day}
      <span className="kx-row-clock">, {time}</span>
    </>
  );
}
