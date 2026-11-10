import { useCallback, useEffect, useRef, useState, type ReactNode } from "react";
import { Cloud as CloudIcon, CloudOff, Copy, LoaderCircle, Lock, RefreshCw } from "lucide-react";
import { cloud, type BudgetSync, type Cloud, type Overview, type PairingOffer, type SyncKeyInfo } from "./cloud";
import { errorText, formatDate } from "./format";
import { Modal } from "./forms";
import { SyncConflicts } from "./SyncConflicts";
import { useToast } from "./ui";
import { msg, t, tn } from "./i18n";
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

/** Sync was unlocked or set up here. `recoveryCode` is there when this made the account's key
 *  without a passphrase: the code is shown once, for the person to keep. */
type Unlocked = (created: boolean, recoveryCode?: string) => void;

/** Sets sync up on the account's first computer, or unlocks it on another. Each case shows one
 *  thing to do: turn sync on, type the passphrase the account has, or use a code. Anything
 *  else is under "Can't unlock?". */
function UnlockSync({ cloud: state, onDone }: { cloud: Cloud; onDone: Unlocked }) {
  const [info, setInfo] = useState<SyncKeyInfo | null>(null);
  const [error, setError] = useState("");
  /** On an account with a passphrase: unlock with a code instead. */
  const [useCode, setUseCode] = useState(false);
  const [resetting, setResetting] = useState(false);
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
  if (!info.exists) return info.setup === "code" ? <TurnOnSync cloud={state} onDone={onDone} /> : <Passphrase cloud={state} exists={false} onDone={onDone} />;
  const byCode = !info.passphrase || useCode;
  return (
    <>
      {byCode ? <CodeUnlock cloud={state} recovery={info.recovery} onDone={() => onDone(false)} /> : <Passphrase cloud={state} exists onDone={onDone} />}
      <details className="unlock-help">
        <summary>{t("Can't unlock?")}</summary>
        <p className="help">
          {info.passphrase && (
            <button type="button" className="link-button" onClick={() => setUseCode(!byCode)}>
              {byCode ? t("Use the sync passphrase instead") : t("Use a code from another device instead")}
            </button>
          )}
          <button type="button" className="link-button" onClick={() => setResetting(true)}>
            {t("Reset sync…")}
          </button>
        </p>
      </details>
      {resetting && <ResetSync cloud={state} generated={info.reset === "code"} locked close={() => setResetting(false)} onDone={(code) => onDone(false, code)} />}
    </>
  );
}

/** Sync's first computer on an account that needs no passphrase: the key is made here, and its
 *  recovery code comes back for the person to keep. */
function TurnOnSync({ cloud: state, onDone }: { cloud: Cloud; onDone: Unlocked }) {
  const toast = useToast();
  const [busy, setBusy] = useState(false);
  const turnOn = async () => {
    setBusy(true);
    try {
      const { created, recoveryCode } = await cloud.syncSetup();
      await state.refresh();
      onDone(created, recoveryCode);
    } catch (err) {
      toast({ message: errorText(err), tone: "error" });
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
      <small className="muted">{t("No passphrase to choose. You'll get a recovery code to keep.")}</small>
    </div>
  );
}

/** The passphrase of an account whose key is made from one: chosen on its first computer
 *  (`exists` false), typed on the others. */
function Passphrase({ cloud: state, exists, onDone }: { cloud: Cloud; exists: boolean; onDone: Unlocked }) {
  const toast = useToast();
  const [passphrase, setPassphrase] = useState("");
  const [again, setAgain] = useState("");
  const [busy, setBusy] = useState(false);
  const mismatch = !exists && again.length > 0 && again !== passphrase;
  return (
    <form
      className="passphrase"
      onSubmit={async (e) => {
        e.preventDefault();
        setBusy(true);
        try {
          const { created } = await cloud.syncSetup(passphrase);
          await state.refresh();
          onDone(created);
        } catch (err) {
          toast({ message: errorText(err), tone: "error" });
          setBusy(false);
        }
      }}
    >
      <p className="help">
        {exists
          ? t("Enter the sync passphrase you chose on your other computer. It decrypts your budgets here.")
          : t("Choose a sync passphrase. Your budgets are encrypted with it on this computer before they're uploaded, so nobody else, including us, can read them. We never see it and can't recover it; if you forget it, you can reset sync from any computer that has your budgets.")}
      </p>
      <label>
        {t("Sync passphrase")}
        <input type="password" value={passphrase} onChange={(e) => setPassphrase(e.target.value)} autoComplete={exists ? "current-password" : "new-password"} minLength={8} required />
      </label>
      {!exists && (
        <label>
          {t("Type it again")}
          <input type="password" value={again} onChange={(e) => setAgain(e.target.value)} autoComplete="new-password" aria-invalid={mismatch} />
          {mismatch && <small className="negative">{t("The two don't match.")}</small>}
        </label>
      )}
      <div>
        <button className="primary" disabled={busy || passphrase.length < 8 || (!exists && again !== passphrase)}>
          {busy && <LoaderCircle size={14} className="spin" />} {exists ? t("Unlock sync") : t("Turn on sync")}
        </button>
      </div>
      {state.status?.credentialStore && (
        <small className="muted">
          {t("This computer keeps the key in {store}, so you enter the passphrase once.", { store: state.status.credentialStore })}
        </small>
      )}
    </form>
  );
}

/** A code as people type it: 24 characters, in any case, with or without the hyphens. */
const codeComplete = (code: string) => code.replace(/[\s-]/g, "").length >= 24;

/** How often a computer showing its own code asks whether a phone has approved it. */
const PAIRING_ASK_MS = 3000;

/** Unlocks sync on this computer with nothing typed from memory. A computer has no camera, so
 *  it shows a code of its own for a phone that syncs to scan and approve. Its one box takes
 *  whichever code the person has instead: the one another device shows under Add a device, or
 *  the account's recovery code (`recovery`: it has one). */
function CodeUnlock({ cloud: state, recovery, onDone }: { cloud: Cloud; recovery: boolean; onDone: () => void }) {
  const toast = useToast();
  const [code, setCode] = useState("");
  const [busy, setBusy] = useState(false);
  const [shown, setShown] = useState<PairingOffer | null>(null);
  const [expired, setExpired] = useState(false);
  const [attempt, setAttempt] = useState(0);
  const finish = useRef({ state, onDone });
  finish.current = { state, onDone };
  useEffect(() => {
    let stopped = false;
    let timer: ReturnType<typeof setTimeout> | undefined;
    setShown(null);
    setExpired(false);
    void cloud.syncPairRequest().then(
      (made) => {
        if (stopped) return;
        setShown(made);
        const ask = async () => {
          if (stopped) return;
          if (Date.parse(made.expiresAt) <= Date.now()) return setExpired(true);
          try {
            if ((await cloud.syncPairWait(made.code)).ready) {
              if (stopped) return;
              stopped = true;
              await finish.current.state.refresh();
              return finish.current.onDone();
            }
          } catch (err) {
            if (stopped) return;
            // The phone's answer arrived and couldn't be used, or the server can't be reached.
            toast({ message: errorText(err), tone: "error" });
            return setExpired(true);
          }
          timer = setTimeout(ask, PAIRING_ASK_MS);
        };
        timer = setTimeout(ask, PAIRING_ASK_MS);
      },
      // No code to show: typing one from another device still works.
      () => {},
    );
    return () => {
      stopped = true;
      clearTimeout(timer);
    };
  }, [attempt]);
  return (
    <form
      className="passphrase"
      onSubmit={async (e) => {
        e.preventDefault();
        setBusy(true);
        try {
          await cloud.syncUnlockCode(code);
          await state.refresh();
          onDone();
        } catch (err) {
          toast({ message: errorText(err), tone: "error" });
        } finally {
          setBusy(false);
        }
      }}
    >
      {expired ? (
        <p className="help">
          {t("This code has expired.")}{" "}
          <button type="button" className="link-button" onClick={() => setAttempt((n) => n + 1)}>
            {t("Make a new code")}
          </button>
        </p>
      ) : (
        shown && (
          <div className="pairing pairing-start">
            <p className="help">
              {t("Scan this with UrBudget on a phone that already syncs: open Account & sync there and choose Add a device.")}
            </p>
            <QrCode qr={shown.qr} label={t("QR code for unlocking sync on this computer")} />
          </div>
        )
      )}
      <label>
        {recovery ? t("Or type a code from another device, or your recovery code") : t("Or type a code from another device")}
        <input
          className="pairing-code-input"
          value={code}
          onChange={(e) => setCode(e.target.value)}
          placeholder="XXXX-XXXX-XXXX-XXXX-XXXX-XXXX"
          autoComplete="off"
          autoCapitalize="characters"
          spellCheck={false}
          dir="ltr"
          required
        />
      </label>
      <div>
        <button className="primary" disabled={busy || !codeComplete(code)}>
          {busy && <LoaderCircle size={14} className="spin" />} {t("Unlock sync")}
        </button>
      </div>
    </form>
  );
}

/** The QR code's squares as one path, with the four-square margin scanners expect. Always dark
 *  on white, whatever the window's theme: a scanner reads contrast, not style. */
function QrCode({ qr, label }: { qr: PairingOffer["qr"]; label: string }) {
  const margin = 4;
  let path = "";
  for (let y = 0; y < qr.size; y++)
    for (let x = 0; x < qr.size; x++)
      if (qr.modules[y * qr.size + x] === "1") path += `M${x + margin} ${y + margin}h1v1h-1z`;
  const side = qr.size + margin * 2;
  return (
    <svg className="pairing-qr" viewBox={`0 0 ${side} ${side}`} role="img" aria-label={label} shapeRendering="crispEdges">
      <rect width={side} height={side} fill="#fff" />
      <path d={path} fill="#000" />
    </svg>
  );
}

/** On a computer that syncs: shows a one-time code that hands the sync key to another device.
 *  The code is taken back when this closes, and stops working by itself after five minutes. */
function AddDevice({ close }: { close: () => void }) {
  const [offer, setOffer] = useState<PairingOffer | null>(null);
  const [error, setError] = useState("");
  const [expired, setExpired] = useState(false);
  const [attempt, setAttempt] = useState(0);
  useEffect(() => {
    let id = "";
    let closed = false;
    let timer: ReturnType<typeof setTimeout> | undefined;
    setOffer(null);
    setError("");
    setExpired(false);
    void cloud.syncPairOffer().then(
      (made) => {
        if (closed) return void cloud.syncPairCancel(made.id).catch(() => {});
        id = made.id;
        setOffer(made);
        timer = setTimeout(() => setExpired(true), Math.max(0, Date.parse(made.expiresAt) - Date.now()));
      },
      (e) => {
        if (!closed) setError(errorText(e));
      },
    );
    return () => {
      closed = true;
      clearTimeout(timer);
      if (id) void cloud.syncPairCancel(id).catch(() => {});
    };
  }, [attempt]);
  return (
    <Modal title={t("Add a device")} close={close}>
      {error ? (
        <p className="notice">
          <span>{error}</span>
        </p>
      ) : !offer ? (
        <LoaderCircle size={16} className="spin" />
      ) : expired ? (
        <p>{t("This code has expired.")}</p>
      ) : (
        <div className="pairing">
          <p>{t("Sign in to UrBudget on the other device, then scan this code there or type it.")}</p>
          <QrCode qr={offer.qr} label={t("QR code for adding a device")} />
          <code className="pairing-code" dir="ltr">
            {offer.code}
          </code>
          <p className="muted">
            {t("It works once, for 5 minutes. Only use it on a device of your own: anyone who has it can read your budgets, and UrBudget will never ask you for it.")}
          </p>
        </div>
      )}
      <div className="modal-actions">
        {(error || expired) && (
          <button className="secondary" onClick={() => setAttempt((n) => n + 1)}>
            {t("Make a new code")}
          </button>
        )}
        <button className="primary" data-autofocus onClick={close}>
          {t("Done")}
        </button>
      </div>
    </Modal>
  );
}

/** Settings › Account & sync. */
export function SyncSettings({
  cloud: state,
  openBudget,
  budgetId,
  isClean,
}: {
  cloud: Cloud;
  budgetId: string;
  /** The open budget has no unsaved edits. */
  isClean: () => boolean;
  openBudget: (id: string) => Promise<void>;
}) {
  const toast = useToast();
  const [overview, setOverview] = useState<Overview | null>(null);
  const [overviewError, setOverviewError] = useState("");
  const overviewRequest = useRef(0);
  const [confirm, setConfirm] = useState<null | { title: string; text: ReactNode; label: string; danger?: boolean; action: () => Promise<void> }>(null);
  const [resetting, setResetting] = useState(false);
  const [adding, setAdding] = useState(false);
  const [recovery, setRecovery] = useState(false);
  /** A recovery code just made, shown until the person says they've saved it. */
  const [newCode, setNewCode] = useState("");
  const [busyId, setBusyId] = useState("");
  const [reviewing, setReviewing] = useState<BudgetSync | null>(null);
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

  if (!status)
    return (
      <section className="panel">
        <h2>{t("Account")}</h2>
        <LoaderCircle size={16} className="spin" />
      </section>
    );
  const report = status.sync.last;
  const subscribed = overview?.sync.active ?? report?.state === "ok";
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
        {status.sync.protocol !== 2 && b.state === "local_only" && (
          <button className="secondary" disabled={!!busyId} onClick={() => void act(b.id, () => cloud.syncTrack([b.id]))}>
            {t("Sync this budget")}
          </button>
        )}
        {status.sync.protocol !== 2 && b.state === "cloud_only" && (
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
        {status.sync.protocol !== 2 && ["synced", "uploaded", "downloaded", "merged", "waiting", "conflict", "cloud_only", "error"].includes(b.state) && (
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

  return (
    <>
      <section className="panel">
        <h2>{t("Account")}</h2>
        {status.email ? (
          <div className="setting-list">
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
                <button className="secondary" onClick={() => void cloud.open("/account/").catch(fail)}>
                  {t("Manage account")}
                </button>
                <DeleteAccount cloud={state} />
                <button
                  className="secondary"
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
          </div>
        ) : (
          <>
            <p className="help">{t("An account is optional: you need one only for sync. Budgeting never needs it.")}</p>
            <SignIn cloud={state} />
          </>
        )}
      </section>

      <section className="panel sync-panel">
        {/* For an account that syncs the older way. One that hasn't set sync up has nothing to keep syncing. */}
        {status.email && status.sync.protocol !== 2 && status.sync.unlocked && <p className="help">{t("Automatic budget discovery is not enabled for this account yet. Existing budgets keep syncing; use the controls below to add or download budgets.")}</p>}
        <div className="panel-title">
          <h2>
            <CloudIcon size={18} /> {t("Sync")}
          </h2>
          {status.email && status.sync.unlocked && subscribed && (
            <button className="secondary" disabled={state.syncing} onClick={() => void state.syncNow()}>
              <RefreshCw size={14} className={state.syncing ? "spin" : ""} /> {t("Sync now")}
            </button>
          )}
        </div>
        <p className="help">
          {t("Keeps your budgets the same on all your devices. End-to-end encrypted: they're encrypted on your computer before upload, and nobody else, including us, can read them. $3/month, cancel anytime.")}
        </p>
        {!status.email ? (
          <p className="muted">{t("Sign in above to use sync.")}</p>
        ) : overviewError && !overview ? (
          <p className="notice error">
            <CloudOff size={14} /> <span>{overviewError}</span>
            <button className="secondary" onClick={loadOverview}>
              {t("Try again")}
            </button>
          </p>
        ) : !overview && !report ? (
          <LoaderCircle size={16} className="spin" />
        ) : !subscribed ? (
          <div className="plan-offer">
            <div>
              <strong className="plan-price">{t("$3 a month")}</strong>
              <small>
                {overview?.subscription?.status === "canceled"
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
        ) : !status.sync.unlocked ? (
          <UnlockSync
            cloud={state}
            onDone={(created, recoveryCode) => {
              if (recoveryCode) setNewCode(recoveryCode);
              toast({
                // `status` is from before this: an account just set up wasn't on protocol 2 in it yet.
                message: created && (recoveryCode || status.sync.protocol === 2) ? t("Sync is on. Your budgets are uploading, encrypted.") : t("Sync is unlocked on this computer."),
                tone: "success",
              });
              void state.syncNow();
            }}
          />
        ) : (
          <>
            {overview?.subscription && (
              <p className="muted plan-line">
                {overview.subscription.endsAt
                  ? t("Your plan ends on {date}.", { date: formatDate(overview.subscription.endsAt.slice(0, 10)) })
                  : overview.subscription.status === "past_due"
                    ? t("The last payment didn't go through; sync keeps working while it's retried. Update your card under Manage account.")
                    : overview.subscription.renewsAt
                      ? t("Renews on {date}.", { date: formatDate(overview.subscription.renewsAt.slice(0, 10)) })
                      : ""}
              </p>
            )}
            {report && report.state !== "ok" && (
              <p className="notice">
                <span>
                  {report.state === "offline"
                    ? t("Offline. Sync picks up when you're back online.")
                    : report.state === "locked"
                      ? t("Unlock sync on this computer to sync.")
                      : report.message ?? t("Sync couldn't finish.")}
                </span>
              </p>
            )}
            <div className="setting-list">{report?.budgets.map(budgetRow)}</div>
            <p className="muted sync-footer">
              {report?.state === "ok" ? t("Last synced {when}.", { when: ago(report.at) }) : ""}{" "}
              <button className="link-button" onClick={() => setAdding(true)}>
                {t("Add a device…")}
              </button>
            </p>
            {/* What few people need, and nobody often. */}
            <details className="sync-more">
              <summary>{t("More")}</summary>
              <p className="muted sync-footer">
                {status.sync.protocol === 2 && (
                  <button className="link-button" onClick={() => setRecovery(true)}>
                    {t("Recovery code…")}
                  </button>
                )}
                <button className="link-button" onClick={() => setResetting(true)}>
                  {status.sync.protocol === 2 ? t("Reset sync…") : t("Reset passphrase…")}
                </button>
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
              </p>
            </details>
          </>
        )}
      </section>
      {confirm && (
        <Confirm title={confirm.title} label={confirm.label} danger={confirm.danger} action={confirm.action} close={() => setConfirm(null)}>
          {confirm.text}
        </Confirm>
      )}
      {resetting && <ResetSync cloud={state} generated={status.sync.protocol === 2} close={() => setResetting(false)} onDone={(code) => code && setNewCode(code)} />}
      {adding && <AddDevice close={() => setAdding(false)} />}
      {recovery && <RecoveryCodeSettings cloud={state} close={() => setRecovery(false)} onCode={setNewCode} />}
      {newCode && <RecoveryCode code={newCode} close={() => setNewCode("")} />}
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

/** A recovery code that was just made, shown once. It isn't kept on this computer, so the
 *  dialog stays until the person says they've saved it. */
function RecoveryCode({ code, close }: { code: string; close: () => void }) {
  const toast = useToast();
  return (
    // The one way out is the button that says it's saved. Escape and a click outside do
    // nothing, and there's no close button: a slip would lose the only showing.
    <Modal title={t("Save your recovery code")} close={() => {}} className="must-save">
      <div className="pairing">
        <p>{t("If you ever have no device that syncs, this code gets your budgets back. Keep it somewhere safe: we can't show it again.")}</p>
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
        <button className="primary" onClick={close}>
          {t("I've saved it")}
        </button>
      </div>
    </Modal>
  );
}

/** Account & sync › Recovery code, where sync is unlocked: makes a new code. Nothing can show
 *  the current one, which this computer never kept. */
function RecoveryCodeSettings({ cloud: state, close, onCode }: { cloud: Cloud; close: () => void; onCode: (code: string) => void }) {
  const toast = useToast();
  const [busy, setBusy] = useState(false);
  const sync = state.status?.sync;
  const make = async () => {
    setBusy(true);
    try {
      const { recoveryCode } = await cloud.syncRecoveryCreate();
      await state.refresh();
      close();
      onCode(recoveryCode);
    } catch (err) {
      toast({ message: errorText(err), tone: "error" });
      setBusy(false);
    }
  };
  return (
    <Modal title={t("Recovery code")} close={close}>
      <p>{t("A recovery code unlocks sync on a new device when you have no other device to add it from.")}</p>
      {sync?.recovery ? (
        <p>{t("Your account has one, which was shown when it was made and can't be shown again. If you've lost it, or someone else may have seen it, make a new one. The old code stops working and nothing else changes.")}</p>
      ) : (
        <p>{t("Your account doesn't have one yet. Your sync passphrase keeps working after you make one.")}</p>
      )}
      <div className="modal-actions">
        <button className="secondary" onClick={close} disabled={busy}>
          {t("Cancel")}
        </button>
        <button className="primary" disabled={busy} onClick={() => void make()}>
          {busy && <LoaderCircle size={14} className="spin" />} {sync?.recovery ? t("Make a new recovery code") : t("Make a recovery code")}
        </button>
      </div>
    </Modal>
  );
}

/** A new sync key. `generated`: the key is made here and comes with a recovery code, which
 *  `onDone` is given to show; otherwise the person chooses a new passphrase. `locked`: asked
 *  from a computer that can't unlock sync, as the way to start over. */
function ResetSync({ cloud: state, generated, locked, close, onDone }: { cloud: Cloud; generated: boolean; locked?: boolean; close: () => void; onDone: (recoveryCode?: string) => void }) {
  const toast = useToast();
  const [passphrase, setPassphrase] = useState("");
  const [again, setAgain] = useState("");
  const [busy, setBusy] = useState(false);
  return (
    <Modal title={generated ? t("Reset sync?") : t("Choose a new sync passphrase")} close={close}>
      <form
        className="passphrase"
        onSubmit={async (e) => {
          e.preventDefault();
          setBusy(true);
          try {
            const { recoveryCode } = await cloud.syncReset(generated ? undefined : passphrase);
            await state.refresh();
            void state.syncNow();
            toast({ message: generated ? t("Sync was reset.") : t("New passphrase set. This computer's budgets are uploading again."), tone: "success" });
            close();
            onDone(recoveryCode);
          } catch (err) {
            toast({ message: errorText(err), tone: "error" });
            setBusy(false);
          }
        }}
      >
        {generated ? (
          <>
            <p>{t("Resetting makes a new sync key and a new recovery code, and deletes the cloud copies made with the old key.")}</p>
            <p>{t("Budgets stay on the devices that have them. On each device they then show under Budgets as recovered copies, to bring back and add to sync again. Your other devices need to be added again.")}</p>
            {locked && <p>{t("If another device of yours still syncs, add this computer from it instead: nothing is deleted that way.")}</p>}
          </>
        ) : (
          <>
            <p>
              {t("The cloud copies made with the old passphrase are deleted, and this computer's budgets upload again. Your other computers keep their budgets and need the new passphrase to sync.")}
            </p>
            <label>
              {t("New passphrase")}
              <input type="password" value={passphrase} onChange={(e) => setPassphrase(e.target.value)} autoComplete="new-password" minLength={8} required />
            </label>
            <label>
              {t("Type it again")}
              <input type="password" value={again} onChange={(e) => setAgain(e.target.value)} autoComplete="new-password" />
            </label>
          </>
        )}
        <div className="modal-actions">
          <button type="button" className="secondary" onClick={close} disabled={busy}>
            {t("Cancel")}
          </button>
          <button className="danger" disabled={busy || (!generated && (passphrase.length < 8 || passphrase !== again))}>
            {busy && <LoaderCircle size={14} className="spin" />} {t("Reset sync")}
          </button>
        </div>
      </form>
    </Modal>
  );
}
