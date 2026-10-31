import { Download, LoaderCircle, RefreshCw } from "lucide-react";
import { usePref } from "./prefs";
import type { Updates } from "./updates";
import { t } from "./i18n";

/** At 100%, Windows still needs to confirm that the package is ready. */
const downloading = (share: number | null, store: boolean) =>
  share === null
    ? t("Downloading")
    : store && share >= 1
      ? t("Waiting for Microsoft Store…")
      : t("Downloading {percent}%", { percent: Math.round(share * 100) });

/** What's waiting: a version from the update server, or the Store's update (no version known). */
function headline({ available, ready }: Updates) {
  if (!available) return "";
  if (available.version)
    return ready
      ? t("Version {version} is ready to install", { version: available.version })
      : t("Update available: version {version}", { version: available.version });
  return ready
    ? t("Update ready from Microsoft Store")
    : t("Update available from Microsoft Store");
}

/** In the title bar while a newer version is waiting; opens Settings › General. */
export function UpdateChip({
  updates,
  onOpen,
}: {
  updates: Updates;
  onOpen: () => void;
}) {
  if (!updates.available) return null;
  const busy =
    updates.phase === "downloading" || updates.phase === "installing";
  return (
    <button
      className="update-chip"
      title={headline(updates)}
      onClick={onOpen}
    >
      {busy ? (
        <LoaderCircle size={13} className="spin" />
      ) : (
        <Download size={13} />
      )}
      <span className="header-label">
        {updates.phase === "downloading"
          ? downloading(updates.progress, updates.channel === "microsoftStore")
          : t("Update")}
      </span>
    </button>
  );
}

/** Settings › General: the version running, and a newer one to install. */
export function UpdateSetting({
  updates,
  version,
  onInstall,
}: {
  updates: Updates;
  version: string;
  onInstall: () => void;
}) {
  const { available, phase } = updates;
  const [automatic] = usePref("autoUpdate");
  const store = updates.channel === "microsoftStore";
  return (
    <div className="setting-row update-setting">
      <span>
        <strong>{available ? headline(updates) : t("Updates")}</strong>
        {available ? (
          <>
            <small className="update-notes">
              {/* The Store's own notes are the app's words (updates.ts); a release's notes stay as written. */}
              {available.notes
                ? t(available.notes)
                : t("Installing saves your budget first, then UrBudget restarts.")}
            </small>
            {updates.ready && (
              <small>
                {!automatic
                  ? t("It installs when you choose. Your budget is saved first.")
                  : !updates.automatic
                    ? t("It didn't install by itself last time, so it waits for you. Your budget is saved first.")
                    : store
                      ? t("UrBudget installs it when you close it, or after it sits unused for a while and then reopens. Your budget is saved first.")
                      : t("UrBudget installs it after it sits unused for a while, and reopens. Your budget is saved first.")}
              </small>
            )}
          </>
        ) : (
          <small>
            {[
              version && t("You have version {version}.", { version }),
              ...(updates.channel === "development"
                ? [t("This development build doesn't look for updates.")]
                : store
                  ? [
                      updates.current && t("Microsoft Store reports no updates available for this copy."),
                      automatic
                        ? t("UrBudget checks the Store at launch and daily, downloads updates in the background, and installs them when you close it or leave it unused.")
                        : t("UrBudget checks the Store at launch and daily, downloads updates in the background, and lets you install them when you choose."),
                    ]
                  : [
                      updates.current && t("You're up to date."),
                      automatic
                        ? t("UrBudget looks for updates when it starts and once a day, downloads them in the background, and installs them after it sits unused for a while.")
                        : t("UrBudget looks for updates when it starts and once a day, downloads them in the background, and lets you install them when you choose."),
                    ]),
            ]
              .filter(Boolean)
              .join(" ")}
          </small>
        )}
        {updates.error && <small className="form-error">{updates.error}</small>}
      </span>
      <div className="update-actions">
        {available ? (
          <button
            className="primary"
            disabled={phase !== "idle"}
            onClick={onInstall}
          >
            {phase === "idle" ? (
              <Download size={14} />
            ) : (
              <LoaderCircle size={14} className="spin" />
            )}
            {phase === "downloading"
              ? downloading(updates.progress, store)
              : phase === "installing"
                ? t("Installing…")
                : updates.ready
                  ? t("Restart to update")
                  : updates.channel === "microsoftStore"
                    ? t("Install Store update")
                    : t("Install & restart")}
          </button>
        ) : (
          <button
            className="secondary"
            disabled={phase !== "idle"}
            onClick={() => void updates.check(true)}
          >
            {phase === "checking" ? (
              <LoaderCircle size={14} className="spin" />
            ) : (
              <RefreshCw size={14} />
            )}
            {phase === "checking" ? t("Checking…") : t("Check now")}
          </button>
        )}
        {updates.channel === "microsoftStore" && (
          <button
            className="secondary"
            disabled={phase === "installing"}
            onClick={() => void updates.openStore()}
          >
            {t("Open Microsoft Store")}
          </button>
        )}
      </div>
    </div>
  );
}

/** Windows can still finish an install while cancellation is pending, so keep edits locked. */
export function UpdateInstalling({ updates, onClose }: { updates: Updates; onClose: () => void }) {
  const status = updates.installStatus;
  const stopping = status?.stage === "stopping";
  return (
    <>
      <p role="status">{stopping
        ? t("Microsoft Store is taking longer than expected. Your budget is saved. Windows is stopping the update; you can close UrBudget or open Microsoft Store while it finishes.")
        : updates.channel === "microsoftStore"
          ? t("Your budget is saved first, then Windows installs the update, which closes UrBudget and opens it again. Windows may ask for permission first.")
          : t("Your budget is saved first, then UrBudget restarts on the new version.")}</p>
      {status?.stage === "installing" && status.progress !== null && (
        <p>{t("Installing {percent}%", { percent: Math.round(status.progress * 100) })}</p>
      )}
      <LoaderCircle className="spin" />
      {stopping && (
        <div className="modal-actions">
          <button className="secondary" onClick={() => void updates.openStore()}>{t("Open Microsoft Store")}</button>
          <button className="primary" onClick={onClose}>{t("Close UrBudget")}</button>
        </div>
      )}
    </>
  );
}
