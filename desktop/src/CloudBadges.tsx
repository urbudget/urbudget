import { Cloud as CloudIcon, CloudAlert, CloudOff, LoaderCircle } from "lucide-react";
import type { Cloud, SyncReport } from "./cloud";
import { ago } from "./CloudSettings";
import { msg, t } from "./i18n";

export function syncLabel(report: SyncReport | null | undefined, activeId?: string, pending = false): string {
  if (!report) return pending ? msg("Waiting to sync") : msg("Sync");
  const active = activeId ? report.budgets.find((b) => b.id === activeId) : undefined;
  if (report.state === "offline") return msg("Offline");
  if (report.budgets.some((b) => ["conflict", "error", "update_app", "recovery", "subscription_required", "deletion_conflict"].includes(b.state))) return msg("Sync needs you");
  if (report.state === "waiting" || report.budgets.some(b => b.state === "queued")) return msg("Waiting to sync");
  if (report.budgets.some(b => ["paused", "locked", "signed_out"].includes(b.state))) return msg("Sync paused");
  if (report.state !== "ok") return msg("Sync needs you");
  if (active?.state === "local_only") return msg("Saved on this device");
  if (pending) return msg("Waiting to sync");
  if (active?.state === "waiting" || report.budgets.some((b) => b.state === "waiting")) return msg("Waiting to sync");
  if (activeId && !active) return msg("Saved on this device");
  if (!activeId && report.budgets.some((b) => b.state === "local_only")) return msg("Saved on this device");
  return report.budgets.length ? msg("Synced") : msg("Sync");
}

export function SyncChip({ cloud, onOpen }: { cloud: Cloud; onOpen: () => void }) {
  const status = cloud.status;
  if (!status?.email) return null;
  const report = status.sync.last;
  const label = !status.sync.unlocked && (status.sync.enabled || report) ? msg("Sync paused") : cloud.syncing ? msg("Syncing…") : syncLabel(report, cloud.activeBudgetId, cloud.pending);
  const trouble = label === msg("Sync needs you") || label === msg("Sync paused");
  const Icon = cloud.syncing ? LoaderCircle : trouble ? CloudAlert : label === msg("Offline") ? CloudOff : CloudIcon;
  return (
    <button className={"sync-chip" + (trouble ? " trouble" : "")} title={label === msg("Synced") && report ? t("Last synced {time}", { time: ago(report.at) }) : t(label)} onClick={onOpen}>
      <Icon size={13} className={cloud.syncing ? "spin" : undefined} />
      <span className="header-label">{t(label)}</span>
    </button>
  );
}
