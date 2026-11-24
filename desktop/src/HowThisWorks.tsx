// The way from the app to the website's getting-started guide: a quiet link on the welcome screen
// and a row in Settings. Opened in the browser as the privacy policy is.
import { ExternalLink } from "lucide-react";
import { cloud } from "./cloud";
import { errorText } from "./format";
import { useToast } from "./ui";
import { t } from "./i18n";

/** The guide's page on urbudget.app. */
export const GUIDE_PATH = "/getting-started/";

function useOpenGuide() {
  const toast = useToast();
  return () => void cloud.open(GUIDE_PATH).catch((e) => toast({ message: errorText(e), tone: "error" }));
}

export function HowThisWorksLink() {
  const open = useOpenGuide();
  return (
    <button type="button" className="link-button" onClick={open}>
      {t("How this works")}
    </button>
  );
}

/** A row in Settings › General's App panel, laid out like the rows above it. */
export function HowThisWorksRow() {
  const open = useOpenGuide();
  return (
    <div className="setting-row">
      <span>
        <strong>{t("How this works")}</strong>
        <small>{t("A short guide on urbudget.app: envelope budgeting in five steps, a first budget worked through, and a weekly routine.")}</small>
      </span>
      <button className="secondary" onClick={open}>
        <ExternalLink size={14} /> {t("Open the guide")}
      </button>
    </div>
  );
}
