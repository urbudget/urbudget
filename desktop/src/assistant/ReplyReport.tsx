import { useState } from "react";
import { createPortal } from "react-dom";
import { LoaderCircle } from "lucide-react";
import { Modal } from "../forms";
import { errorText } from "../format";
import { assistant, type Provider } from "./api";
import { t } from "../i18n";

/** The most the reason of a report holds, the details that go in with it included. */
const REASON = 1000;
// Do not split an emoji's surrogate pair at the native JSON boundary.
const cut = (text: string, length: number) => text.slice(0, length).replace(/[\uD800-\uDBFF]$/, "");

/**
 * The line of details a report starts with, for the user to edit or clear. It isn't translated:
 * it is read by whoever reviews the report. A version the app couldn't read is left out.
 */
export function reportDetails({
  version,
  provider,
  model,
  tools,
  language,
}: {
  version?: string;
  provider: Provider;
  model?: string | null;
  /** The tools the reply's turn had called. */
  tools?: string[];
  language: string;
}) {
  const service = provider === "chatgpt" ? "ChatGPT" : provider === "openai" ? "OpenAI" : "Anthropic";
  return [
    `App: UrBudget desktop${version ? ` ${version}` : ""}`,
    `Service: ${service}`,
    `Model: ${model || "default"}`,
    `Tools: ${tools?.length ? tools.join(", ") : "none"}`,
    `Language: ${language}`,
  ].join(" · ");
}

/**
 * The reason as it is sent: the user's words, then the details after a blank line. Together
 * they fit the reason's limit by cutting the details short, never the user's words.
 */
export function reasonWith(reason: string, details: string) {
  const words = reason.trim();
  const extra = details.trim();
  if (!extra) return words;
  if (!words) return cut(extra, REASON);
  const room = REASON - words.length - 2;
  const fits = room > 0 ? cut(extra, room).trimEnd() : "";
  return fits ? `${words}\n\n${fits}` : words;
}

/** Reporting is an explicit, editable submission; never send the rest of a conversation. */
export function ReplyReport({ text, details: about = "", close }: { text: string; details?: string; close: () => void }) {
  const [excerpt, setExcerpt] = useState(() => cut(text, 4000));
  const [reason, setReason] = useState("");
  const [details, setDetails] = useState(about);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const [reference, setReference] = useState("");
  return createPortal(
    <Modal title={t("Report this reply")} close={() => { if (!busy) close(); }}>
      {reference ? (
        <div className="reply-report">
          <p role="status">{t("Report sent. Thank you for helping us review the assistant's replies.")}</p>
          <p className="help">
            {t("Keep this reference if you want to ask us to delete the report: {reference}", { reference: <code>{reference}</code> })}
          </p>
          <div className="form-actions"><button className="primary" onClick={close}>{t("Done")}</button></div>
        </div>
      ) : (
        <form className="reply-report" onSubmit={async (event) => {
          event.preventDefault();
          if (busy || !excerpt.trim()) return;
          setBusy(true);
          setError("");
          try {
            setReference(await assistant.reportReply(excerpt.trim(), reasonWith(reason, details)));
          } catch (failure) {
            setError(errorText(failure));
          } finally {
            setBusy(false);
          }
        }}>
          <p className="help">
            {t("Send the text below to UrBudget for review. Remove names, balances or anything private first. Reports are deleted after 90 days.")}{" "}
            {t("Only what you leave in these fields is sent: the reply's text, your explanation and the details.")}
          </p>
          <label className="field-wide">
            {t("Reply to include")}
            <textarea data-autofocus rows={7} maxLength={4000} value={excerpt} disabled={busy}
              onChange={(event) => setExcerpt(event.target.value)} />
          </label>
          {text.length > 4000 && <p className="help">{t("The first 4,000 characters are included. You can replace them with the part you want to report.")}</p>}
          <label className="field-wide">
            {t("What's wrong? (optional)")}
            <textarea rows={3} maxLength={REASON} value={reason} disabled={busy}
              onChange={(event) => setReason(event.target.value)} />
          </label>
          <label className="field-wide">
            {t("Details to include")}
            <textarea rows={2} maxLength={REASON} value={details} disabled={busy}
              onChange={(event) => setDetails(event.target.value)} />
          </label>
          <p className="help">{t("Sent with your report. Remove anything you'd rather not send.")}</p>
          {error && <p className="notice notice-error" role="alert">{error}</p>}
          <div className="form-actions">
            <button type="button" className="secondary" disabled={busy} onClick={close}>{t("Cancel")}</button>
            <button className="primary" disabled={busy || !excerpt.trim() || excerpt.trim().length > 4000 || reason.trim().length > REASON}>
              {busy && <LoaderCircle size={14} className="spin" />} {busy ? t("Sending…") : t("Send report")}
            </button>
          </div>
        </form>
      )}
    </Modal>,
    document.body,
  );
}
