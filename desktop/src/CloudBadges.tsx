import { Cloud as CloudIcon, CloudAlert, CloudOff, LoaderCircle } from "lucide-react";
import type { Cloud } from "./cloud";
import { ago } from "./CloudSettings";
import { t } from "./i18n";

/** In the title bar once sync is set up on this computer; opens Settings › Account & sync. */
export function SyncChip({ cloud, onOpen }: { cloud: Cloud; onOpen: () => void }) {
  const status = cloud.status;
  if (!status?.email || !status.sync.unlocked) return null;
  const report = status.sync.last;
  const trouble = report?.budgets.some((b) => b.state === "conflict" || b.state === "error" || b.state === "update_app");
  const [icon, label] = cloud.syncing
    ? [<LoaderCircle key="i" size={13} className="spin" />, t("Syncing…")]
    : !report
      ? [<CloudIcon key="i" size={13} />, t("Sync")]
      : report.state === "offline"
        ? [<CloudOff key="i" size={13} />, t("Offline")]
        : report.state !== "ok" || trouble
          ? [<CloudAlert key="i" size={13} />, report.state === "subscription_required" ? t("Sync paused") : t("Sync needs you")]
          : [<CloudIcon key="i" size={13} />, t("Synced")];
  return (
    <button
      className={`sync-chip${trouble || (report && !["ok", "offline"].includes(report.state)) ? " trouble" : ""}`}
      title={report?.state === "ok" ? t("Last synced {time}", { time: ago(report.at) }) : label}
      onClick={onOpen}
    >
      {icon}
      <span className="header-label">{label}</span>
    </button>
  );
}
