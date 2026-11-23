// The way from the app to the website's getting-started guide: a quiet link on the welcome screen
// and a panel in Settings. Opened in the browser as the privacy policy is.
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

export function HowThisWorksPanel() {
  const open = useOpenGuide();
  return (
    <section className="panel">
      <h2>{t("How this works")}</h2>
      <p>{t("A short guide on urbudget.app: envelope budgeting in five steps, a first budget worked through, and a weekly routine.")}</p>
      <button className="secondary" onClick={open}>
        {t("Open the guide")}
      </button>
    </section>
  );
}
