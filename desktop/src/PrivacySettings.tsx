import { cloud } from "./cloud";
import { errorText } from "./format";
import { usePref } from "./prefs";
import { useToast } from "./ui";
import { t } from "./i18n";

export function PrivacySettings() {
  const toast = useToast();
  const [usageCounts, setUsageCounts] = usePref("usageCounts");
  return <section className="panel">
    <h2>{t("Privacy & data")}</h2>
    <p>{t("Learn what stays on your device and what optional sync, bank connections, purchases and the assistant share. To delete your account and cloud budgets, open Account & sync and choose Delete account.")}</p>
    <label className="setting-row toggle">
      <span>
        <strong>{t("Count this copy as in use")}</strong>
        <small>{t("Once a day, tells urbudget.app that a copy of UrBudget was opened, and whether that's its first time, or its first this week or month, with its version, system, where it was installed from and the week it was first opened. Nothing identifies you, this computer or your budgets.")}</small>
      </span>
      <input type="checkbox" role="switch" checked={usageCounts} onChange={(e) => setUsageCounts(e.target.checked)} />
    </label>
    <button className="secondary" onClick={() => void cloud.open("/privacy/")
      .catch(e => toast({ message: errorText(e), tone: "error" }))}>{t("Privacy policy")}</button>
  </section>;
}
