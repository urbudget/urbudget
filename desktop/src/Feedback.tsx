// Settings' way to write to support: a new email in the computer's mail app, which Rust starts
// (host.rs) with only the app and its version in the subject.
import { Mail } from "lucide-react";
import { invoke } from "@tauri-apps/api/core";
import { useToast } from "./ui";
import { t } from "./i18n";

/** Where feedback goes. The row shows it, for a computer with no mail app set up. */
export const SUPPORT = "support@urbudget.app";

/** A row in Settings › General's App panel, laid out like the rows above it. */
export function SendFeedbackRow() {
  const toast = useToast();
  const write = () =>
    void invoke<void>("feedback_open").catch(() =>
      toast({ message: t("Couldn't open your mail app. Write to {address}.", { address: SUPPORT }), tone: "error" }),
    );
  return (
    <div className="setting-row">
      <span>
        <strong>{t("Send feedback")}</strong>
        <small>{t("Tell us what works and what doesn't, by email to {address}.", { address: SUPPORT })}</small>
      </span>
      <button className="secondary" onClick={write}>
        <Mail size={14} /> {t("Write an email")}
      </button>
    </div>
  );
}
