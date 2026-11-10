import { useEffect, useRef, useState } from "react";
import { createPortal, flushSync } from "react-dom";
import { createRoot, type Root } from "react-dom/client";
import { FileDown, LoaderCircle, X } from "lucide-react";
import { errorText, formatDate } from "../format";
import { assistant, type ReportView } from "./api";
import { Markdown } from "./Markdown";
import "./report-doc.css";
import documentCss from "./report-doc.css?inline";
import "./report.css";
import { t, tc } from "../i18n";

/** The report as a document: dark ink on white, the same on screen and on paper. */
export function ReportDocument({ report }: { report: ReportView }) {
  const details = [report.period, t("Prepared {date}", { date: formatDate(report.created.slice(0, 10)) })].filter(Boolean).join(" · ");
  return (
    <article className="report-doc">
      <header className="report-head">
        <span className="report-budget">{report.budget}</span>
        <h1>{report.title}</h1>
        <p>{details}</p>
      </header>
      <div className="report-body">
        <Markdown text={report.body} />
      </div>
      <footer className="report-foot">
        {t("Made with UrBudget. Figures come from the budget as it was when this report was written.")}
      </footer>
    </article>
  );
}

let printing: { root: Root; host: HTMLElement; title: string } | null = null;
function endPrint() {
  if (!printing) return;
  printing.root.unmount();
  printing.host.remove();
  document.title = printing.title;
  document.body.classList.remove("printing-report");
  printing = null;
}

/** The report as standalone HTML, drawn by the same component as the preview. */
function reportHtml(report: ReportView) {
  const host = document.createElement("div");
  const root = createRoot(host);
  flushSync(() => root.render(<ReportDocument report={report} />));
  const html = host.innerHTML;
  root.unmount();
  return html;
}
const escapeHtml = (s: string) => s.replace(/[&<>"]/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;" })[c]!);

/**
 * Chromium engines (WebView2 on Windows) print a frame on its own. The report goes in a frame
 * off screen, so the app's page is never changed while the print dialog is open.
 */
function printInFrame(report: ReportView) {
  document.querySelectorAll("iframe.report-frame").forEach((f) => f.remove());
  const frame = document.createElement("iframe");
  frame.className = "report-frame";
  frame.title = report.title;
  frame.tabIndex = -1;
  frame.setAttribute("aria-hidden", "true");
  // Laid out, but off screen: a frame that isn't laid out has nothing to print.
  frame.style.cssText = "position:fixed;left:-10000px;top:0;width:794px;height:1123px;border:0";
  frame.srcdoc =
    `<!doctype html><html lang="${document.documentElement.lang || "en"}" dir="${document.documentElement.dir || "ltr"}"><head><meta charset="utf-8">` +
    `<title>${escapeHtml(report.title)}</title><style>html,body{margin:0;background:#fff;color-scheme:light}${documentCss}</style>` +
    `</head><body>${reportHtml(report)}</body></html>`;
  const title = document.title;
  // The suggested file name comes from the page's title.
  document.title = report.title;
  return new Promise<void>((resolve) => {
    frame.addEventListener(
      "load",
      () => {
        const w = frame.contentWindow;
        if (!w) {
          document.title = title;
          return resolve();
        }
        w.addEventListener(
          "afterprint",
          () => {
            document.title = title;
            setTimeout(() => frame.remove(), 500);
          },
          { once: true },
        );
        w.focus();
        w.print();
        resolve();
      },
      { once: true },
    );
    document.body.appendChild(frame);
  });
}

/** Where printing a frame works: Chromium engines, not WebKit. */
const framePrinting = () => typeof navigator !== "undefined" && /Chrome\//.test(navigator.userAgent);

/**
 * Opens the system print dialog with only the report to print. Save as PDF is one of its
 * printers, and the report's title becomes the file's suggested name.
 */
export async function printReport(report: ReportView, mode: "frame" | "page" = framePrinting() ? "frame" : "page") {
  endPrint();
  if (mode === "frame") return printInFrame(report);
  // WebKit (macOS, Linux) prints the window natively: show only the report on the page.
  const host = document.createElement("div");
  host.className = "report-print-root";
  document.body.appendChild(host);
  const root = createRoot(host);
  flushSync(() => root.render(<ReportDocument report={report} />));
  printing = { root, host, title: document.title };
  document.title = report.title;
  document.body.classList.add("printing-report");
  window.addEventListener("afterprint", endPrint, { once: true });
  try {
    await assistant.print();
  } catch {
    window.print();
  }
}

/** A full-size preview of a report, to print or save as a PDF. */
export function ReportPreview({ threadId, proposalId, onClose }: { threadId: string; proposalId: string; onClose: () => void }) {
  const [report, setReport] = useState<ReportView | null>(null);
  const [error, setError] = useState("");
  const close = useRef(onClose);
  close.current = onClose;
  const dialog = useRef<HTMLDivElement>(null);
  // Whatever had the keyboard when the preview opened (the card's Preview button) gets it back.
  const opener = useRef(typeof document === "undefined" ? null : (document.activeElement as HTMLElement | null));
  const controls = () => [...(dialog.current?.querySelectorAll<HTMLElement>("button:not(:disabled)") ?? [])];
  useEffect(() => {
    void assistant.report(threadId, proposalId).then(setReport).catch((e) => setError(errorText(e)));
    const key = (e: KeyboardEvent) => {
      if (e.key === "Escape") close.current();
      // The keyboard stays in the preview: Tab goes round its controls.
      if (e.key !== "Tab") return;
      const stops = controls();
      const at = stops.indexOf(document.activeElement as HTMLElement);
      if (at >= 0 && at !== (e.shiftKey ? 0 : stops.length - 1)) return;
      e.preventDefault();
      stops[e.shiftKey ? stops.length - 1 : 0]?.focus();
    };
    window.addEventListener("keydown", key);
    return () => window.removeEventListener("keydown", key);
  }, [threadId, proposalId]);
  useEffect(() => {
    const from = opener.current;
    return () => {
      if (from?.isConnected) from.focus();
    };
  }, []);
  // Its first control that can be used takes the keyboard: Close while the report is on its way,
  // then Print once it's there (unless the user has already gone somewhere else in it).
  useEffect(() => {
    const active = document.activeElement as HTMLElement | null;
    const stops = controls();
    if (!dialog.current?.contains(active) || (report && active === stops[stops.length - 1])) stops[0]?.focus();
  }, [report, error]);
  return createPortal(
    <div className="report-shade" onMouseDown={(e) => e.target === e.currentTarget && onClose()}>
      <div ref={dialog} className="report-window" role="dialog" aria-modal="true" aria-label={report?.title ?? tc("noun", "Report")}>
        <div className="report-toolbar">
          <strong>{report?.title ?? tc("noun", "Report")}</strong>
          <span className="report-toolbar-hint">{t("Preview")}</span>
          <button className="primary small" disabled={!report} onClick={() => report && void printReport(report)}>
            <FileDown size={14} aria-hidden /> {t("Print or save as PDF")}
          </button>
          <button className="icon-button" aria-label={t("Close the preview")} title={t("Close (Esc)")} onClick={onClose}>
            <X size={16} />
          </button>
        </div>
        <div className="report-scroll">
          {report ? (
            <div className="report-paper">
              <ReportDocument report={report} />
            </div>
          ) : error ? (
            <p className="report-error" role="alert">
              {error}
            </p>
          ) : (
            <LoaderCircle size={20} className="spin" />
          )}
        </div>
      </div>
    </div>,
    document.body,
  );
}
