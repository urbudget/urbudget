import { useCallback, useEffect, useId, useRef, useState, type ReactNode } from "react";
import { Cloud as CloudIcon, CloudOff, Copy, LoaderCircle, Lock, Plus, RefreshCw } from "lucide-react";
import { cloud, type BudgetSync, type Cloud, type Overview, type PairingCode, type SyncDevice, type SyncKeyInfo, type SyncUnlocked } from "./cloud";
import { errorText, formatDate } from "./format";
import { Modal } from "./forms";
import { SyncConflicts } from "./SyncConflicts";
import { useToast } from "./ui";
import { localDate, localDateTime, msg, t, tc, tn } from "./i18n";
import "./cloud.css";

/** "just now", "5 minutes ago", "3 hours ago", or the date. */
export function ago(iso: string | null | undefined) {
  if (!iso) return "";
  const seconds = (Date.now() - Date.parse(iso)) / 1000;
  if (!Number.isFinite(seconds)) return "";
  if (seconds < 60) return t("just now");
  if (seconds < 3600) return tn(Math.round(seconds / 60), "{count} min ago", "{count} min ago");
  if (seconds < 86_400) return tn(Math.round(seconds / 3600), "{count} h ago", "{count} h ago");
  return formatDate(iso.slice(0, 10));
}

function Confirm({
  title,
  children,
  label,
  danger,
  action,
  close,
}: {
  title: string;
  children: ReactNode;
  label: string;
  danger?: boolean;
  action: () => Promise<void>;
  close: () => void;
}) {
  const toast = useToast();
  const [busy, setBusy] = useState(false);
  return (
    <Modal title={title} close={close}>
      {children}
      <div className="modal-actions">
        <button className="secondary" onClick={close} disabled={busy}>
          {t("Cancel")}
        </button>
        <button
          className={danger ? "danger" : "primary"}
          disabled={busy}
          onClick={async () => {
            setBusy(true);
            try {
              await action();
              close();
            } catch (e) {
              toast({ message: errorText(e), tone: "error" });
              setBusy(false);
            }
          }}
        >
          {busy && <LoaderCircle size={14} className="spin" />} {label}
        </button>
      </div>
    </Modal>
  );
}

function DeleteAccount({ cloud: state }: { cloud: Cloud }) {
  const [open, setOpen] = useState(false);
  const [confirm, setConfirm] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const email = state.status?.email ?? "";
  return <>
    <button className="secondary danger-text" onClick={() => { setConfirm(""); setError(""); setOpen(true); }}>{t("Delete account")}</button>
    {open && <Modal title={t("Delete your UrBudget account?")} close={() => { if (!busy) setOpen(false); }}>
      <p>{t("This deletes your account and cloud budgets and stops subscription renewals. Budgets saved on your devices stay there. Purchase records may be retained as described in the privacy policy.")}</p>
      <label>{t("Type {email} to confirm", { email })}<input type="email" value={confirm} onChange={e => setConfirm(e.target.value)} autoComplete="off" disabled={busy} /></label>
      {error && <p role="alert" className="notice error">{error}</p>}
      <div className="modal-actions">
        <button className="secondary" disabled={busy} onClick={() => setOpen(false)}>{t("Cancel")}</button>
        <button className="danger" disabled={busy || !email || confirm.trim().toLowerCase() !== email.toLowerCase()} onClick={async () => {
          setBusy(true); setError("");
          try { state.setStatus(await cloud.deleteAccount(confirm)); setOpen(false); }
          catch (e) { setError(errorText(e)); }
          finally { setBusy(false); }
        }}>{busy ? t("Deleting…") : t("Delete account and cloud data")}</button>
      </div>
    </Modal>}
  </>;
}

function SignIn({ cloud: state }: { cloud: Cloud }) {
  const toast = useToast();
  const [email, setEmail] = useState("");
  const [code, setCode] = useState("");
  const [sent, setSent] = useState(false);
  const [reviewAccess, setReviewAccess] = useState(false);
  const [busy, setBusy] = useState(false);
  const fail = (e: unknown) => toast({ message: errorText(e), tone: "error" });
  const send = async () => {
    setBusy(true);
    try {
      const answer = await cloud.start(email);
      setReviewAccess(answer.reviewAccess === true);
      setCode(answer.reviewAccess ? "" : answer.devCode ?? "");
      setSent(true);
    } catch (e) {
      fail(e);
    } finally {
      setBusy(false);
    }
  };
  const verify = async () => {
    setBusy(true);
    try {
      state.setStatus(await cloud.verify(email, code));
    } catch (e) {
      fail(e);
    } finally {
      setBusy(false);
    }
  };
  return (
    <form
      className="sign-in"
      onSubmit={(e) => {
        e.preventDefault();
        void (sent ? verify() : send());
      }}
    >
      {!sent ? (
        <label>
          {t("Email")}
          <span className="inline-field">
            <input type="email" value={email} onChange={(e) => setEmail(e.target.value)} placeholder="you@example.com" autoComplete="email" disabled={busy} required />
            <button className="primary" disabled={busy || !email.includes("@")}>
              {busy && <LoaderCircle size={14} className="spin" />} {t("Email me a code")}
            </button>
          </span>
        </label>
      ) : (
        <label>
          {reviewAccess ? t("Review access code") : t("The 6-digit code we sent to {email}", { email })}
          <span className="inline-field">
            <input
              className="mono code-input"
              type={reviewAccess ? "password" : "text"}
              value={code}
              onChange={(e) => setCode(reviewAccess ? e.target.value.replace(/[^A-Za-z0-9_-]/g, "").slice(0, 128) : e.target.value.replace(/\D/g, "").slice(0, 6))}
              inputMode={reviewAccess ? "text" : "numeric"}
              autoComplete={reviewAccess ? "off" : "one-time-code"}
              minLength={reviewAccess ? 24 : 6}
              maxLength={reviewAccess ? 128 : 6}
              placeholder={reviewAccess ? t("Access code") : "123456"}
              spellCheck={false}
              autoFocus
            />
            <button className="primary" disabled={busy || (reviewAccess ? code.length < 24 : code.length !== 6)}>
              {busy && <LoaderCircle size={14} className="spin" />} {t("Sign in")}
            </button>
            <button type="button" className="link-button" disabled={busy} onClick={() => { setSent(false); setCode(""); setReviewAccess(false); }}>
              {t("Use another email")}
            </button>
          </span>
        </label>
      )}
      <small className="muted">
        {sent
          ? reviewAccess ? t("Use the access code supplied with the app review instructions.") : t("It can take a minute to arrive. Not there? Check your Junk or Spam folder.")
          : t("No password: each time you sign in, we email you a code.")}
      </small>
    </form>
  );
}

const BUDGET_STATES: Record<BudgetSync["state"], string> = {
  synced: msg("Synced"),
  uploaded: msg("Synced"),
  downloaded: msg("Updated from the cloud"),
  merged: msg("Synced, changes from both combined"),
  conflict: msg("Changed in two places"),
  cloud_only: msg("In the cloud"),
  local_only: msg("Only on this computer"),
  waiting: msg("Waiting to sync"),
  update_app: msg("Needs a newer UrBudget"),
  error: msg("Couldn't sync"),
  recovery: msg("Recovered local changes"),
  trashed: msg("In Trash"),
  deletion_conflict: msg("Newer changes need review before deletion"),
  purged: msg("Permanently deleted"),
  queued: msg("Waiting to sync"),
  signed_out: msg("Sign in to sync"),
  evicted: msg("Available online"),
  locked: msg("Unlock sync"),
  paused: msg("Sync paused"),
  subscription_required: msg("Subscription required"),
};

/** Sync was set up, unlocked or reset on this computer. */
type Unlocked = (answer: SyncUnlocked) => void;

/** How often a screen showing a code asks whether the other device has used it. */
const PAIR_ASK_MS = 3000;

/** 24 code characters as people type them: any case, with or without the hyphens. */
const codeTyped = (text: string) => text.replace(/[\s-]/g, "").length >= 24;

/** Sync on a computer that doesn't have the key: turns it on for an account with no key, and
 *  unlocks it for one that has. Which is asked fresh, since another device may have turned
 *  sync on since this computer last heard. */
function UnlockSync({ cloud: state, onDone, reset }: { cloud: Cloud; onDone: Unlocked; reset: () => void }) {
  const [info, setInfo] = useState<SyncKeyInfo | null>(null);
  const [error, setError] = useState("");
  const load = useCallback(() => {
    setError("");
    void cloud.syncKeyInfo().then(setInfo, (e) => setError(errorText(e)));
  }, []);
  useEffect(load, [load]);
  if (error)
    return (
      <p className="notice error">
        <CloudOff size={14} /> <span>{error}</span>
        <button className="secondary" onClick={load}>
          {t("Try again")}
        </button>
      </p>
    );
  if (!info) return <LoaderCircle size={16} className="spin" />;
  return info.exists ? <Locked cloud={state} secret={info.secret} onDone={onDone} reset={reset} /> : <TurnOnSync cloud={state} onDone={onDone} taken={load} />;
}

/** The account's first device: nothing to choose or type. The key is made here, from a recovery
 *  code that comes back to be shown. */
function TurnOnSync({ cloud: state, onDone, taken }: { cloud: Cloud; onDone: Unlocked; taken: () => void }) {
  const toast = useToast();
  const [busy, setBusy] = useState(false);
  const turnOn = async () => {
    setBusy(true);
    try {
      const answer = await cloud.syncSetup();
      // A status that can't be read just now mustn't cost the showing of the code.
      await state.refresh().catch(() => null);
      onDone(answer);
    } catch (e) {
      toast({ message: errorText(e), tone: "error" });
      // Another device may have turned sync on meanwhile: then this one unlocks it instead.
      taken();
    } finally {
      setBusy(false);
    }
  };
  return (
    <div className="passphrase">
      <div>
        <button className="primary" disabled={busy} onClick={() => void turnOn()}>
          {busy && <LoaderCircle size={14} className="spin" />} {t("Turn on sync")}
        </button>
      </div>
      <small className="muted">{t("You'll get a recovery code to keep somewhere safe.")}</small>
    </div>
  );
}

/** Sync has a key and this computer doesn't. A computer has no camera, so it shows a code of
 *  its own for a phone that syncs to scan and approve, and asks until the phone has. Its one
 *  box takes what the person has instead: a code another device shows, the recovery code, or
 *  the passphrase of an account that has one (`secret`). */
function Locked({ cloud: state, secret, onDone, reset }: { cloud: Cloud; secret: SyncKeyInfo["secret"]; onDone: Unlocked; reset: () => void }) {
  const toast = useToast();
  const [typed, setTyped] = useState("");
  const [visible, setVisible] = useState(false);
  const [busy, setBusy] = useState(false);
  const [shown, setShown] = useState<PairingCode | null>(null);
  /** Why the code on show is done with: it ran out, or what went wrong asking about it. */
  const [ended, setEnded] = useState("");
  const [attempt, setAttempt] = useState(0);
  const latest = useRef({ state, onDone });
  latest.current = { state, onDone };
  useEffect(() => {
    let left = false;
    let id = "";
    let timer: ReturnType<typeof setTimeout> | undefined;
    setShown(null);
    setEnded("");
    void cloud.syncPairRequest().then(
      (made) => {
        if (left) return;
        id = made.id;
        setShown(made);
        const ask = async () => {
          if (left) return;
          if (Date.parse(made.expiresAt) <= Date.now()) return setEnded(t("This code has expired."));
          try {
            const answer = await cloud.syncPairWait(made.code);
            if (left) return;
            if (answer.ready) {
              left = true;
              id = "";
              await latest.current.state.refresh().catch(() => null);
              return latest.current.onDone(answer);
            }
          } catch (e) {
            // The phone's answer couldn't be used, or urbudget.app can't be reached.
            if (!left) setEnded(errorText(e));
            return;
          }
          timer = setTimeout(ask, PAIR_ASK_MS);
        };
        timer = setTimeout(ask, PAIR_ASK_MS);
      },
      // No code to show: typing one still works.
      () => {},
    );
    return () => {
      left = true;
      clearTimeout(timer);
      // Takes back a key a phone sealed for a code this computer no longer asks about.
      if (id) void cloud.syncPairCancel(id).catch(() => {});
    };
  }, [attempt]);
  const passphrase = secret === "passphrase";
  return (
    <div className="unlock">
      {ended ? (
        <p className="help unlock-code">
          {ended}{" "}
          <button type="button" className="link-button" onClick={() => setAttempt((n) => n + 1)}>
            {t("Show a new code")}
          </button>
        </p>
      ) : (
        shown && (
          <div className="pairing unlock-code">
            <p className="help">{t("On a phone that syncs, open Account & sync › Add a device and scan this code.")}</p>
            <ShownCode code={shown} label={t("QR code for unlocking sync on this computer")} />
          </div>
        )
      )}
      <form
        className="passphrase"
        onSubmit={async (e) => {
          e.preventDefault();
          setBusy(true);
          try {
            const answer = await cloud.syncUnlock(typed);
            await state.refresh().catch(() => null);
            onDone(answer);
          } catch (err) {
            toast({ message: errorText(err), tone: "error" });
          } finally {
            setBusy(false);
          }
        }}
      >
        <label>
          {passphrase ? t("Code from another device, your recovery code, or your sync passphrase") : t("Code from another device, or your recovery code")}
          <input
            className="pairing-code-input"
            // A passphrase is hidden as it's typed, and a password manager can fill it in.
            type={passphrase && !visible ? "password" : "text"}
            value={typed}
            onChange={(e) => setTyped(e.target.value)}
            placeholder={passphrase ? undefined : "XXXX-XXXX-XXXX-XXXX-XXXX-XXXX"}
            autoComplete={passphrase ? "current-password" : "off"}
            spellCheck={false}
            dir={passphrase ? "auto" : "ltr"}
            required
          />
        </label>
        {passphrase && (
          <label className="checkbox-label">
            <input type="checkbox" checked={visible} onChange={(e) => setVisible(e.target.checked)} /> {t("Show what I type")}
          </label>
        )}
        <div>
          <button className="primary" disabled={busy || !(passphrase ? typed.length >= 8 : codeTyped(typed))}>
            {busy && <LoaderCircle size={14} className="spin" />} {t("Unlock sync")}
          </button>
        </div>
        {state.status?.credentialStore && (
          <small className="muted">{t("This computer keeps the key in {store}, so you unlock sync here once.", { store: state.status.credentialStore })}</small>
        )}
        <div>
          <button type="button" className="link-button" onClick={reset}>
            {t("Can't unlock?")}
          </button>
        </div>
      </form>
    </div>
  );
}

/** A code as a QR code with its characters under it, which are what a screen reader and a
 *  keyboard get. The squares are one path with the four-square margin scanners expect, always
 *  dark on white whatever the window's theme: a scanner reads contrast, not style. */
function ShownCode({ code, label }: { code: PairingCode; label: string }) {
  const characters = useId();
  const { size, modules } = code.qr;
  const margin = 4;
  let path = "";
  for (let y = 0; y < size; y++)
    for (let x = 0; x < size; x++)
      if (modules[y * size + x] === "1") path += `M${x + margin} ${y + margin}h1v1h-1z`;
  const side = size + margin * 2;
  return (
    <>
      <svg className="pairing-qr" viewBox={`0 0 ${side} ${side}`} role="img" aria-label={label} aria-describedby={characters} shapeRendering="crispEdges">
        <rect width={side} height={side} fill="#fff" />
        <path d={path} fill="#000" />
      </svg>
      <code id={characters} className="pairing-code" dir="ltr">
        {code.code}
      </code>
    </>
  );
}

/** On a computer that syncs: shows a one-time code that hands the sync key to another device,
 *  and closes itself once that device has it. The code is taken back when this closes, and
 *  stops working by itself after five minutes. */
function AddDevice({ close, added }: { close: () => void; added: () => void }) {
  const [offer, setOffer] = useState<PairingCode | null>(null);
  const [error, setError] = useState("");
  const [expired, setExpired] = useState(false);
  const [attempt, setAttempt] = useState(0);
  const done = useRef(added);
  done.current = added;
  useEffect(() => {
    let left = false;
    let id = "";
    let timer: ReturnType<typeof setTimeout> | undefined;
    setOffer(null);
    setError("");
    setExpired(false);
    void cloud.syncPairOffer().then(
      (made) => {
        if (left) return void cloud.syncPairCancel(made.id).catch(() => {});
        id = made.id;
        setOffer(made);
        const ends = Date.parse(made.expiresAt);
        const ask = async () => {
          if (left) return;
          if (ends <= Date.now()) return setExpired(true);
          // A missed answer is asked for again: the code is still good.
          const state = await cloud.syncPairState(made.id).catch(() => null);
          if (left) return;
          if (state?.collected) {
            id = "";
            return done.current();
          }
          if (state && !state.waiting) return setExpired(true);
          timer = setTimeout(ask, PAIR_ASK_MS);
        };
        timer = setTimeout(ask, PAIR_ASK_MS);
      },
      (e) => {
        if (!left) setError(errorText(e));
      },
    );
    return () => {
      left = true;
      clearTimeout(timer);
      if (id) void cloud.syncPairCancel(id).catch(() => {});
    };
  }, [attempt]);
  return (
    <Modal title={t("Add a device")} close={close}>
      {error ? (
        <p role="alert" className="notice error">
          {error}
        </p>
      ) : expired ? (
        <p>{t("This code has expired.")}</p>
      ) : !offer ? (
        <LoaderCircle size={16} className="spin" />
      ) : (
        <div className="pairing">
          <p>{t("Scan this code with the new phone, or type its characters on the new computer where it asks to unlock sync.")}</p>
          <ShownCode code={offer} label={t("QR code for adding a device")} />
          <p className="muted">
            {t("It works once, until {time}. Use it only on a device you're holding: anyone with this code can read your budgets.", {
              time: localDateTime(new Date(offer.expiresAt), { timeStyle: "short" }),
            })}
          </p>
        </div>
      )}
      <div className="modal-actions">
        <button className="secondary" data-autofocus onClick={close}>
          {t("Cancel")}
        </button>
        {(error || expired) && (
          <button className="primary" onClick={() => setAttempt((n) => n + 1)}>
            {t("Show a new code")}
          </button>
        )}
      </div>
    </Modal>
  );
}

/** A recovery code this computer still holds, up until the person says it's saved. */
function RecoveryCode({ code, saved }: { code: string; saved: () => Promise<void> }) {
  const toast = useToast();
  const [busy, setBusy] = useState(false);
  return (
    // The one way out is the button that says it's saved: Escape and a click outside do
    // nothing, and the heading's close button is hidden (cloud.css).
    <Modal title={t("Save your recovery code")} close={() => {}} className="must-save">
      <div className="pairing">
        <p>{t("This code unlocks sync on a new device when you have no other device at hand. Keep it somewhere safe: UrBudget can't show it again or recover it.")}</p>
        <code className="pairing-code recovery-code" dir="ltr">
          {code}
        </code>
        <button
          className="secondary"
          onClick={() =>
            void navigator.clipboard.writeText(code).then(
              () => toast({ message: t("Recovery code copied."), tone: "success" }),
              () => toast({ message: t("Couldn't copy the code. Select it and copy it yourself."), tone: "error" }),
            )
          }
        >
          <Copy size={14} /> {t("Copy")}
        </button>
      </div>
      <div className="modal-actions">
        <button
          className="primary"
          disabled={busy}
          onClick={async () => {
            setBusy(true);
            try {
              await saved();
            } catch (e) {
              toast({ message: errorText(e), tone: "error" });
              setBusy(false);
            }
          }}
        >
          {t("I've saved it")}
        </button>
      </div>
    </Modal>
  );
}

/**
 * An account that still syncs on protocol 1 while urbudget.app offers the new sync. Moving is the
 * person's choice because devices on older versions stop syncing until they're updated.
 */
function MoveToNewSync({ cloud: state }: { cloud: Cloud }) {
  const toast = useToast();
  const [open, setOpen] = useState(false);
  const [passphrase, setPassphrase] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const needsPassphrase = state.status?.sync.upgradeNeedsPassphrase === true;
  return (
    <div className="plan-offer sync-move">
      <div>
        <strong>{t("A new sync is ready for this account")}</strong>
        <small>
          {state.status?.sync.secret === "code"
            ? t("It finds every budget in your account on its own and recovers safely when an upload is interrupted. Your budgets and recovery code stay the same.")
            : t("It finds every budget in your account on its own and recovers safely when an upload is interrupted. Your budgets and sync passphrase stay the same.")}
        </small>
      </div>
      <span className="sync-actions">
        <button className="primary" onClick={() => { setPassphrase(""); setError(""); setOpen(true); }}>
          {t("Move to the new sync")}
        </button>
      </span>
      {open && (
        <Modal title={t("Move to the new sync?")} close={() => { if (!busy) setOpen(false); }}>
          <form
            className="passphrase"
            onSubmit={async (e) => {
              e.preventDefault();
              setBusy(true);
              setError("");
              try {
                await cloud.syncUpgrade(needsPassphrase ? passphrase : undefined);
                setOpen(false);
                toast({ message: t("This account now uses the new sync."), tone: "success" });
                await state.refresh();
                void state.syncNow();
              } catch (err) {
                setError(errorText(err));
                // A key from before a passphrase reset elsewhere turns this into a passphrase request.
                await state.refresh().catch(() => null);
              } finally {
                setBusy(false);
              }
            }}
          >
            <p>{t("After the move, every device that syncs with this account needs desktop 1.0.11 or later, Android 1.0.1 or later, or iPhone 1.0.1 or later. Older versions are told to update UrBudget before they sync again; the budgets on them stay.")}</p>
            {needsPassphrase && (
              <label>
                {t("Sync passphrase")}
                <input type="password" value={passphrase} onChange={(e) => setPassphrase(e.target.value)} autoComplete="current-password" minLength={8} required disabled={busy} autoFocus />
              </label>
            )}
            {error && <p role="alert" className="notice error">{error}</p>}
            <div className="modal-actions">
              <button type="button" className="secondary" onClick={() => setOpen(false)} disabled={busy}>
                {t("Cancel")}
              </button>
              <button className="primary" disabled={busy || (needsPassphrase && passphrase.length < 8)}>
                {busy && <LoaderCircle size={14} className="spin" />} {t("Move to the new sync")}
              </button>
            </div>
          </form>
        </Modal>
      )}
    </div>
  );
}

/** Budgets that sync with nothing for anyone to do: counted, not listed. */
const SYNCING: readonly string[] = ["synced", "uploaded", "downloaded", "merged", "waiting", "queued"];
/** Budgets with nothing to do from this page: the budget library has them. */
const SHELVED: readonly string[] = ["trashed", "purged", "evicted"];

/** Settings › Account & sync: the account, the plan when it needs something, sync, the devices
 *  that sync, the budgets that need someone, and what few people need under More. */
export function SyncSettings({
  cloud: state,
  openBudget,
  budgetId,
  isClean,
  manageBudgets,
}: {
  cloud: Cloud;
  budgetId: string;
  /** The open budget has no unsaved edits. */
  isClean: () => boolean;
  openBudget: (id: string) => Promise<void>;
  /** Opens the budget library, where every budget is listed. */
  manageBudgets?: () => void;
}) {
  const toast = useToast();
  const [overview, setOverview] = useState<Overview | null>(null);
  const [overviewError, setOverviewError] = useState("");
  const overviewRequest = useRef(0);
  const [confirm, setConfirm] = useState<null | { title: string; text: ReactNode; label: string; danger?: boolean; action: () => Promise<void> }>(null);
  const [adding, setAdding] = useState(false);
  /** A recovery code this computer holds until the person says they've saved it. */
  const [recoveryCode, setRecoveryCode] = useState("");
  const [busyId, setBusyId] = useState("");
  const [reviewing, setReviewing] = useState<BudgetSync | null>(null);
  /** The devices that sync with the account, this computer first. Null until they're known. */
  const [devices, setDevices] = useState<SyncDevice[] | null>(null);
  const [devicesError, setDevicesError] = useState("");
  const devicesRequest = useRef(0);
  const status = state.status;
  const email = status?.email;
  const currentEmail = useRef(email);
  currentEmail.current = email;
  const fail = (e: unknown) => toast({ message: errorText(e), tone: "error" });
  const loadOverview = useCallback(() => {
    if (!email) return;
    const request = ++overviewRequest.current;
    const current = () => request === overviewRequest.current && email === currentEmail.current;
    setOverviewError("");
    void cloud
      .overview()
      .then((next) => { if (current()) setOverview(next); })
      .catch((e) => { if (current()) setOverviewError(errorText(e)); });
  }, [email]);
  useEffect(() => {
    setOverview(null);
    setOverviewError("");
    loadOverview();
    // Coming back from the browser (after subscribing) shows the new plan.
    window.addEventListener("focus", loadOverview);
    return () => { overviewRequest.current++; window.removeEventListener("focus", loadOverview); };
  }, [loadOverview]);
  // A code made here that nobody confirmed saving is shown again, whenever this page opens:
  // the app may have closed, or the answer that carried it may have been lost.
  const unsaved = status?.sync.recoveryUnsaved === true;
  useEffect(() => setRecoveryCode(""), [email]);
  useEffect(() => {
    if (!email || !unsaved) return;
    let left = false;
    void cloud.syncRecoveryUnsaved().then((kept) => { if (!left && kept) setRecoveryCode(kept.recoveryCode); }, () => {});
    return () => { left = true; };
  }, [email, unsaved]);

  const report = status?.sync.last ?? null;
  const subscribed = overview?.sync.active ?? report?.state === "ok";
  /** Where the page is: signed out, the account couldn't be read, still reading it, no plan,
   *  sync off or locked on this computer, or sync on. */
  const phase = !email ? "out" : overviewError && !overview ? "error" : !overview && !report ? "loading" : !subscribed ? "plan" : !status?.sync.unlocked ? "locked" : "on";
  const on = phase === "on";
  // The list is asked for while sync is on here, and again after a device is added.
  const loadDevices = useCallback(() => {
    if (!on || !email) return;
    const request = ++devicesRequest.current;
    setDevicesError("");
    void cloud.syncDevices().then(
      (answer) => { if (request === devicesRequest.current) setDevices(answer.devices); },
      (e) => { if (request === devicesRequest.current) setDevicesError(errorText(e)); },
    );
  }, [on, email]);
  useEffect(() => {
    setDevices(null);
    setDevicesError("");
    loadDevices();
    return () => { devicesRequest.current++; };
  }, [loadDevices]);

  if (!status)
    return (
      <section className="panel">
        <h2>{t("Account")}</h2>
        <LoaderCircle size={16} className="spin" />
      </section>
    );
  const older = status.sync.protocol !== 2;
  const plan = overview?.subscription ?? null;
  const manageAccount = () => void cloud.open("/account/").catch(fail);
  const act = async (id: string, work: () => Promise<unknown>) => {
    setBusyId(id);
    try {
      await work();
      await state.syncNow();
    } catch (e) {
      fail(e);
    } finally {
      setBusyId("");
    }
  };

  const unlockedHere = (answer: SyncUnlocked) => {
    if (answer.recoveryCode) setRecoveryCode(answer.recoveryCode);
    toast({ message: answer.created ? t("Sync is on. Your budgets are uploading, encrypted.") : t("Sync is unlocked on this computer."), tone: "success" });
    void state.syncNow();
  };
  /** `locked`: asked from a computer that can't unlock sync, as the way to start over. */
  const askReset = (locked: boolean) =>
    setConfirm({
      title: t("Reset sync?"),
      text: (
        <>
          {state.status?.sync.protocol === 2 ? (
            <>
              <p>{t("Sync gets a new key and a new recovery code, and the cloud copies made with the old key are deleted.")}</p>
              <p>{t("This computer uploads the budgets it was syncing again, and they keep syncing.")}</p>
              <p>{t("Your other devices are unlocked with the new recovery code, or added from this computer.")}</p>
              <p>{t("Each of them then receives those budgets and keeps its own copy in the budget library. Recover as a copy brings back anything only that device had.")}</p>
            </>
          ) : (
            <>
              <p>{t("Sync gets a new key and a new recovery code. The cloud copies made with the old key are deleted, and this computer's budgets upload again.")}</p>
              <p>{t("Your other devices keep their budgets. To sync again, each one is unlocked with the new recovery code or added from this computer.")}</p>
            </>
          )}
          {locked && <p>{t("If another device of yours still syncs, add this computer from it instead. Nothing is deleted that way.")}</p>}
        </>
      ),
      label: t("Reset sync"),
      danger: true,
      action: async () => {
        const { recoveryCode } = await cloud.syncReset();
        if (recoveryCode) setRecoveryCode(recoveryCode);
        await state.refresh().catch(() => null);
        void state.syncNow();
        toast({ message: t("Sync was reset."), tone: "success" });
      },
    });
  const askNewCode = () =>
    setConfirm({
      title: t("Make a new recovery code?"),
      text: (
        <p>
          {status.sync.recovery
            ? t("The new code replaces the last one made this way. The code or passphrase sync was set up with keeps working. To retire it, reset sync.")
            : t("The code or passphrase you already have keeps working. To retire it, reset sync.")}
        </p>
      ),
      label: t("Make a new code"),
      action: async () => {
        setRecoveryCode((await cloud.syncRecoveryCreate()).recoveryCode);
        await state.refresh().catch(() => null);
      },
    });

  /** A budget is listed when it needs someone, or has something to tell them. One that is
   *  simply syncing is counted instead, and one in Trash or left online is the library's. */
  const listed = (b: BudgetSync) =>
    SYNCING.includes(b.state) ? !!b.message || !!b.notes?.length : SHELVED.includes(b.state) ? false : b.state === "cloud_only" ? older : true;
  const budgets = on ? (report?.budgets ?? []) : [];
  const needing = budgets.filter(listed);
  const quiet = budgets.filter((b) => SYNCING.includes(b.state) && !listed(b));
  const counted = tn(quiet.length, "{count} budget is syncing.", "{count} budgets are syncing.");

  const budgetRow = (b: BudgetSync) => (
    <div className="setting-row sync-budget" key={b.id}>
      <span>
        <strong>
          {b.name}
          {b.id === budgetId && <em className="muted"> · {t("open")}</em>}
        </strong>
        <small className={`sync-state ${b.state}`}>
          {t(BUDGET_STATES[b.state] ?? msg("Sync needs you"))}
          {b.message
            ? ` · ${b.message}`
            : b.conflicts?.length
              ? ` · ${tn(b.conflicts.length, "{count} change to choose", "{count} changes to choose")}`
              : b.updatedAt && b.state !== "local_only"
                ? ` · ${
                    b.device
                      ? t("last change {when} from {device}", { when: ago(b.updatedAt), device: b.device })
                      : t("last change {when}", { when: ago(b.updatedAt) })
                  }`
                : ""}
        </small>
        {b.notes?.map((note) => (
          <small key={note} className="sync-note">
            {note}
          </small>
        ))}
      </span>
      <span className="sync-actions">
        {busyId === b.id && <LoaderCircle size={14} className="spin" />}
        {b.state === "local_only" && (
          <button className="secondary" disabled={!!busyId} onClick={() => void act(b.id, () => (status.sync.protocol === 2 ? cloud.syncAdd([b.id]) : cloud.syncTrack([b.id])))}>
            {t("Sync this budget")}
          </button>
        )}
        {older && b.state === "cloud_only" && (
          <button
            className="secondary"
            disabled={!!busyId}
            onClick={() =>
              void act(b.id, async () => {
                await cloud.syncDownload(b.id);
                toast({ message: t("Downloaded “{name}”.", { name: b.name }), action: { label: t("Open it"), onClick: () => void openBudget(b.id).catch(fail) } });
              })
            }
          >
            {t("Download")}
          </button>
        )}
        {b.state === "conflict" && !!b.conflicts?.length && (
          <button className="secondary" disabled={!!busyId} onClick={() => setReviewing(b)}>
            {t("Review")}
          </button>
        )}
        {b.state === "conflict" && !b.conflicts?.length && (
          <>
            <button
              className="secondary"
              disabled={!!busyId}
              onClick={() =>
                setConfirm({
                  title: t("Keep this computer's version?"),
                  text: <p>{t("“{name}” as it is here replaces the cloud copy. Your other computers get this version on their next sync.", { name: b.name })}</p>,
                  label: t("Keep this computer's"),
                  action: () => act(b.id, () => cloud.syncResolve(b.id, "local", isClean(), undefined, b.review)),
                })
              }
            >
              {t("Keep this computer's")}
            </button>
            <button
              className="secondary"
              disabled={!!busyId}
              onClick={() =>
                setConfirm({
                  title: t("Use the cloud copy?"),
                  text: (
                    <p>
                      {b.device
                        ? t("“{name}” here is replaced by the cloud copy from {device}. This computer's version is backed up first (Settings › Backup & export).", { name: b.name, device: b.device })
                        : t("“{name}” here is replaced by the cloud copy. This computer's version is backed up first (Settings › Backup & export).", { name: b.name })}
                    </p>
                  ),
                  label: t("Use the cloud copy"),
                  action: () => act(b.id, () => cloud.syncResolve(b.id, "cloud", isClean(), undefined, b.review)),
                })
              }
            >
              {t("Use the cloud copy")}
            </button>
          </>
        )}
        {/* A kept copy is recovered, and a deletion reviewed, in the budget library. */}
        {manageBudgets && (b.state === "recovery" || b.state === "deletion_conflict") && (
          <button className="secondary" disabled={!!busyId} onClick={manageBudgets}>
            {t("Manage budgets…")}
          </button>
        )}
        {older && ["synced", "uploaded", "downloaded", "merged", "waiting", "conflict", "cloud_only", "error"].includes(b.state) && (
          <button
            className="link-button"
            disabled={!!busyId}
            onClick={() =>
              setConfirm({
                title: t("Stop syncing “{name}”?", { name: b.name }),
                text: <p>{t("Its cloud copy is deleted. The copies on your computers stay where they are.")}</p>,
                label: t("Stop syncing"),
                danger: true,
                action: () => act(b.id, () => cloud.syncRemove(b.id)),
              })
            }
          >
            {t("Stop syncing")}
          </button>
        )}
      </span>
    </div>
  );

  const deviceRow = (d: SyncDevice) => (
    <li className="setting-row" key={d.id}>
      <span>
        <strong>{d.name || (d.current ? t("This computer") : t("A device added on {date}", { date: localDate(new Date(d.addedAt), { day: "numeric", month: "long" }) }))}</strong>
        {d.current ? d.name && <small>{t("This computer")}</small> : d.lastSeenAt && <small>{t("Last synced {time}", { time: ago(d.lastSeenAt) })}</small>}
      </span>
    </li>
  );

  /** What sync does and which secret opens the budgets: said before sync is on, and kept under More after. */
  const about =
    status.sync.secret === "passphrase"
      ? t("Keeps your budgets the same on all your devices. End-to-end encrypted: they're encrypted on your computer before upload, and only your devices and your sync passphrase can open them. $3/month, cancel anytime.")
      : t("Keeps your budgets the same on all your devices. End-to-end encrypted: they're encrypted on your computer before upload, and only your devices and your recovery code can open them. $3/month, cancel anytime.");
  /** Sync's one line while it's on: that it is and when it last ran, or what it's waiting for. */
  const trouble = report?.state === "error" || report?.state === "attention";
  const line = !report
    ? t("Sync is on.")
    : report.state === "ok"
      ? t("Sync is on. Last synced {when}.", { when: ago(report.at) })
      : report.state === "offline"
        ? t("Offline. Sync picks up when you're back online.")
        : report.state === "locked"
          ? t("Unlock sync on this computer to sync.")
          : report.message
            ? report.message
            : report.state === "waiting"
              ? t("Sync is on. Changes are waiting to sync.")
              : report.state === "attention" && needing.length
                ? tn(needing.length, "Sync is on. {count} budget needs you.", "Sync is on. {count} budgets need you.")
                : t("Sync couldn't finish.");

  return (
    <>
      <section className="panel">
        <h2>{t("Account")}</h2>
        {status.email ? (
          <div className="setting-row">
            <span>
              <strong>{t("Signed in as {email}", { email: status.email })}</strong>
              <small>
                {status.credentialStore
                  ? t("The sign-in is kept in {store}.", { store: status.credentialStore })
                  : t("You'll sign in again next time you open UrBudget.")}
              </small>
            </span>
            <span className="sync-actions">
              <button
                className="link-button"
                onClick={() =>
                  setConfirm({
                    title: t("Sign out of UrBudget?"),
                    text: <p>{t("Sync stops on this computer and it forgets the sync key. Your budgets here stay as they are.")}</p>,
                    label: t("Sign out"),
                    action: async () => {
                      state.setStatus(await cloud.signOut());
                      setOverview(null);
                    },
                  })
                }
              >
                {t("Sign out")}
              </button>
            </span>
          </div>
        ) : (
          <>
            <p className="help">{t("An account is optional: you need one only for sync. Budgeting never needs it.")}</p>
            <SignIn cloud={state} />
          </>
        )}
      </section>

      {/* The plan takes room only when there's something to do or to know about it. */}
      {phase === "plan" ? (
        <section className="panel">
          <h2>{tc("subscription", "Plan")}</h2>
          <div className="plan-offer">
            <div>
              <strong className="plan-price">{t("$3 a month")}</strong>
              <small>
                {plan?.status === "canceled"
                  ? t("Your sync plan has ended. Subscribe again to pick up where you left off.")
                  : t("Subscribe on urbudget.app, then come back here.")}
              </small>
            </div>
            <span className="sync-actions">
              <button className="primary" onClick={() => void cloud.open("/account/?subscribe=1").catch(fail)}>
                {t("Subscribe")}
              </button>
              <button className="link-button" onClick={loadOverview}>
                {t("I've subscribed")}
              </button>
            </span>
          </div>
        </section>
      ) : (
        (phase === "locked" || on) &&
        (plan?.endsAt || plan?.status === "past_due") && (
          <section className="panel">
            <h2>{tc("subscription", "Plan")}</h2>
            <div className="sync-status">
              <span>
                {plan.endsAt
                  ? t("Your plan ends on {date}.", { date: formatDate(plan.endsAt.slice(0, 10)) })
                  : t("The last payment didn't go through; sync keeps working while it's retried. Update your card under Manage account.")}
              </span>
              <span className="sync-actions">
                <button className="secondary" onClick={manageAccount}>
                  {t("Manage account")}
                </button>
              </span>
            </div>
          </section>
        )
      )}

      <section className="panel sync-panel">
        {status.email && older && status.sync.upgradeAvailable && <MoveToNewSync cloud={state} />}
        <div className="panel-title">
          <h2>
            <CloudIcon size={18} /> {t("Sync")}
          </h2>
        </div>
        {(phase === "out" || phase === "plan") && <p className="help">{about}</p>}
        {phase === "out" ? (
          <p className="muted">{t("Sign in above to use sync.")}</p>
        ) : phase === "error" ? (
          <p className="notice error">
            <CloudOff size={14} /> <span>{overviewError}</span>
            <button className="secondary" onClick={loadOverview}>
              {t("Try again")}
            </button>
          </p>
        ) : phase === "loading" ? (
          <LoaderCircle size={16} className="spin" />
        ) : phase === "locked" ? (
          <UnlockSync cloud={state} onDone={unlockedHere} reset={() => askReset(true)} />
        ) : (
          on && (
            <div className={"sync-status" + (trouble ? " trouble" : "")}>
              <span>{line}</span>
              <span className="sync-actions">
                <button className="secondary" disabled={state.syncing} onClick={() => void state.syncNow()}>
                  <RefreshCw size={14} className={state.syncing ? "spin" : ""} /> {t("Sync now")}
                </button>
              </span>
            </div>
          )
        )}
      </section>

      {on && (
        <section className="panel">
          <h2>{t("Devices")}</h2>
          {!!devices?.length && (
            <ul className="setting-list device-list" aria-label={t("Devices")}>
              {devices.map(deviceRow)}
            </ul>
          )}
          {/* An account on the older sync has no list to show: nothing is said about it. */}
          {devicesError && !devices && !older && (
            <p className="muted sync-footer">
              <span>{devicesError}</span>
              <button className="link-button" onClick={loadDevices}>
                {t("Try again")}
              </button>
            </p>
          )}
          <div className="device-add">
            <button className="secondary" onClick={() => setAdding(true)}>
              <Plus size={14} /> {t("Add a device")}
            </button>
          </div>
          <p className="help">{t("To stop a device syncing, sign out on it. To cut off a device you no longer have, reset sync.")}</p>
        </section>
      )}

      {(needing.length > 0 || quiet.length > 0) && (
        <section className="panel">
          <h2>{t("Budgets")}</h2>
          {needing.length > 0 && <div className="setting-list">{needing.map(budgetRow)}</div>}
          {quiet.length > 0 &&
            (older ? (
              // On the older sync each budget is taken out of sync by hand, so the count opens onto them.
              <details className="sync-more">
                <summary>{counted}</summary>
                <div className="setting-list">{quiet.map(budgetRow)}</div>
              </details>
            ) : (
              <p className="muted sync-footer">
                <span>{counted}</span>
                {manageBudgets && (
                  <button className="link-button" onClick={manageBudgets}>
                    {t("Manage budgets…")}
                  </button>
                )}
              </p>
            ))}
        </section>
      )}

      {/* What few people need, and nobody often. */}
      {status.email && (
        <section className="panel more-panel">
          <details className="sync-more">
            <summary>{t("More")}</summary>
            {on && (
              <>
                <p className="help">{about}</p>
                <p className="sync-footer">
                  {/* Protocol 1 keeps no recovery copy: the account moves to the new sync first. */}
                  {!older && (
                    <button className="link-button" onClick={askNewCode}>
                      {t("New recovery code…")}
                    </button>
                  )}
                  <button
                    className="link-button"
                    onClick={() =>
                      setConfirm({
                        title: t("Lock sync on this computer?"),
                        text: <p>{t("This computer forgets the sync key and stops syncing until you unlock sync on it again.")}</p>,
                        label: t("Lock"),
                        action: async () => {
                          await cloud.syncLock();
                          await state.refresh();
                        },
                      })
                    }
                  >
                    <Lock size={12} /> {t("Lock on this computer")}
                  </button>
                  <button className="link-button" onClick={() => askReset(false)}>
                    {t("Reset sync…")}
                  </button>
                </p>
              </>
            )}
            <p className="sync-footer">
              {subscribed && plan?.renewsAt && !plan.endsAt && plan.status !== "past_due" && (
                <span>{t("Renews on {date}.", { date: formatDate(plan.renewsAt.slice(0, 10)) })}</span>
              )}
              <button className="link-button" onClick={manageAccount}>
                {t("Manage account")}
              </button>
              <button className="link-button" onClick={() => void cloud.open("/privacy/").catch(fail)}>
                {t("Privacy policy")}
              </button>
            </p>
            <p className="sync-footer">
              <DeleteAccount cloud={state} />
            </p>
          </details>
        </section>
      )}
      {confirm && (
        <Confirm title={confirm.title} label={confirm.label} danger={confirm.danger} action={confirm.action} close={() => setConfirm(null)}>
          {confirm.text}
        </Confirm>
      )}
      {adding && (
        <AddDevice
          close={() => setAdding(false)}
          added={() => {
            setAdding(false);
            toast({ message: t("Device added."), tone: "success" });
            loadDevices();
          }}
        />
      )}
      {/* After the dialog that asked for it has closed, so the keyboard focus ends up in this one. */}
      {status.email && recoveryCode && !confirm && (
        <RecoveryCode
          code={recoveryCode}
          saved={async () => {
            await cloud.syncRecoverySaved();
            setRecoveryCode("");
            await state.refresh().catch(() => null);
          }}
        />
      )}
      {reviewing && (
        <SyncConflicts
          budget={reviewing}
          close={() => setReviewing(null)}
          resolve={async (choices) => {
            setBusyId(reviewing.id);
            try {
              await cloud.syncResolve(reviewing.id, "merge", isClean(), choices, reviewing.review);
              await state.refresh();
            } finally {
              setBusyId("");
            }
          }}
        />
      )}
    </>
  );
}
