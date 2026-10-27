import { Download, LoaderCircle, RefreshCw } from "lucide-react";
import { usePref } from "./prefs";
import type { Updates } from "./updates";

const percent = (share: number | null) =>
  share === null ? "" : ` ${Math.round(share * 100)}%`;

/** What's waiting: a version from the update server, or the Store's update (no version known). */
function headline({ available, ready }: Updates) {
  if (!available) return "";
  if (available.version)
    return ready
      ? `Version ${available.version} is ready to install`
      : `Update available: version ${available.version}`;
  return ready
    ? "Update ready from Microsoft Store"
    : "Update available from Microsoft Store";
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
          ? `Downloading${percent(updates.progress)}`
          : "Update"}
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
        <strong>{available ? headline(updates) : "Updates"}</strong>
        {available ? (
          <>
            <small className="update-notes">
              {available.notes ||
                "Installing saves your budget first, then UrBudget restarts."}
            </small>
            {updates.ready && (
              <small>
                {!automatic
                  ? "It installs when you choose. Your budget is saved first."
                  : !updates.automatic
                    ? "It didn't install by itself last time, so it waits for you. Your budget is saved first."
                    : store
                      ? "UrBudget installs it when you close it, or after it sits unused for a while and then reopens. Your budget is saved first."
                      : "UrBudget installs it after it sits unused for a while, and reopens. Your budget is saved first."}
              </small>
            )}
          </>
        ) : (
          <small>
            {version && `You have version ${version}. `}
            {updates.channel === "development"
              ? "This development build doesn't look for updates."
              : store
                ? `${updates.current ? "Microsoft Store reports no updates available for this copy. " : ""}UrBudget checks the Store at launch and daily, downloads updates in the background, and ${automatic ? "installs them when you close it or leave it unused" : "lets you install them when you choose"}.`
                : `${updates.current ? "You're up to date. " : ""}UrBudget looks for updates when it starts and once a day, downloads them in the background, and ${automatic ? "installs them after it sits unused for a while" : "lets you install them when you choose"}.`}
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
              ? `Downloading${percent(updates.progress)}`
              : phase === "installing"
                ? "Installing…"
                : updates.ready
                  ? "Restart to update"
                  : updates.channel === "microsoftStore"
                    ? "Install Store update"
                    : "Install & restart"}
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
            {phase === "checking" ? "Checking…" : "Check now"}
          </button>
        )}
        {updates.channel === "microsoftStore" && (
          <button
            className="secondary"
            disabled={phase !== "idle"}
            onClick={() => void updates.openStore()}
          >
            Open Microsoft Store
          </button>
        )}
      </div>
    </div>
  );
}
