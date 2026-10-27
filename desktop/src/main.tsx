import React from "react";
import ReactDOM from "react-dom/client";
// Base styles first, so page stylesheets imported by components can override them.
import "./styles.css";
import App from "./App";
import { ToastProvider, TooltipLayer } from "./ui";
import { describe, installErrorLog, logError } from "./assistant/errors";
import { applyLanguage, t } from "./i18n";

installErrorLog();
// <html lang dir> and the engine's language, before the first paint.
applyLanguage();

class RenderBoundary extends React.Component<
  { children: React.ReactNode },
  { failed: boolean; detail: string }
> {
  state = { failed: false, detail: "" };
  static getDerivedStateFromError(error: unknown) {
    return { failed: true, detail: describe(error) };
  }
  componentDidCatch(error: unknown, info: React.ErrorInfo) {
    logError(error, info.componentStack ?? "");
  }
  render() {
    if (this.state.failed)
      return (
        <div className="loading-screen">
          <h1>{t("Let’s reopen your budget.")}</h1>
          <p>
            {t("The screen encountered an error. Your last database save is safe, and any available recovery draft will be offered when you reload.")}
          </p>
          <button className="primary" onClick={() => location.reload()}>
            {t("Reload application")}
          </button>
          <details className="crash-details">
            <summary>{t("Details")}</summary>
            <pre>{this.state.detail}</pre>
          </details>
        </div>
      );
    return this.props.children;
  }
}
ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <RenderBoundary>
      <ToastProvider>
        <App />
        <TooltipLayer />
      </ToastProvider>
    </RenderBoundary>
  </React.StrictMode>,
);
