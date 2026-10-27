import { useEffect, useRef, useState } from "react";
import { KeyRound, MessageSquareText, Trash2, TriangleAlert } from "lucide-react";
import { api } from "./bridge";
import { formatDate, money, monthName } from "./format";
import { Modal } from "./forms";
import type { ImportDifference, ImportResult, SavedYnabToken, YnabImportRequest, YnabPlan } from "./types";
import { tokenStore as systemStore } from "./platform";
import { ltr, msg, msgc, t, tc, tn } from "./i18n";
import "./import.css";

const FIELDS: Record<string, string> = {
  balance: msg("balance"),
  cleared_balance: msg("cleared balance"),
  uncleared_balance: msg("uncleared balance"),
  budgeted: msg("assigned"),
  activity: msg("activity"),
};
/** A total that differs from YNAB's, in words: "Car Loan balance: −10,600.00 here, −10,831.27 in YNAB". */
export function differenceText(d: ImportDifference) {
  const amounts = t("{actual} here, {expected} in YNAB", { actual: money(d.actual), expected: money(d.expected) });
  const month = d.month ? monthName(d.month.slice(0, 7)) : "";
  if (d.kind === "account") {
    const known = FIELDS[d.field ?? "balance"];
    return t("{account} {field}: {amounts}", { account: String(d.account), field: known ? t(known) : String(d.field), amounts });
  }
  if (d.kind === "category") {
    const known = d.field === "balance" ? msgc("field", "available") : FIELDS[d.field ?? ""];
    return t("{category}, {month}, {field}: {amounts}", {
      category: String(d.category),
      month,
      field: known ? t(known) : String(d.field),
      amounts,
    });
  }
  return t("Ready to Assign, {month}: {amounts}", { month, amounts });
}
const SHOWN = 12;
const CONFLICT_FIELDS: Record<string, string> = {
  splits: msg("split lines"),
  status: msg("cleared"),
  approved: msg("approval"),
  transferStatus: msg("transfer's cleared"),
  accountId: msg("account"),
};
/** A value in the conflict review: the side in words, plain text, an amount as money, or the record's JSON. */
function conflictValue(value: unknown, field?: string | null, text?: string | null) {
  if (value === null || value === undefined) return t("Deleted");
  if (text) return text;
  if (typeof value === "string") return value || t("(empty)");
  if (typeof value === "number") return field === "amount" ? money(value) : String(value);
  if (typeof value === "boolean") return value ? t("Yes") : t("No");
  return JSON.stringify(value, null, 2);
}
/** What the assistant is asked after an import with differences, or one that stopped. */
export function ynabHelpText(plan: string, problem: { differences: ImportDifference[] } | { error: string }) {
  if ("error" in problem) {
    return t(
      "My YNAB import of {plan} stopped with this message: \"{error}\" Explain what it means and what I can change, in YNAB or here, so the import works.",
      { plan, error: problem.error },
    );
  }
  const lines = problem.differences.map((d) => `- ${differenceText(d)}`);
  return [
    t("I imported {plan} from YNAB, but these totals came out different from what YNAB shows:", { plan }),
    ...lines,
    t("Find what's causing each difference by looking at the transactions involved, and suggest how to fix it."),
  ].join("\n");
}

export function YnabImport({
  onImport,
  onAccept,
  onResolve,
  onAsk,
  planId: initialPlan,
  blank = false,
  tokenStore = systemStore,
  listPlans = api.ynabPlans,
  listTokens = api.ynabTokens,
  forgetToken = api.forgetToken,
}: {
  onImport: (request: YnabImportRequest) => Promise<ImportResult>;
  /** Saves an import whose totals differ from YNAB's. */
  onAccept?: (reviewId: string) => Promise<ImportResult>;
  onResolve?: (
    id: string,
    choices: Record<string, string>,
  ) => Promise<ImportResult>;
  /** Opens the assistant with a question. */
  onAsk?: (text: string) => void;
  planId?: string;
  /** The budget has nothing in it yet, so there's nothing to merge with or replace. */
  blank?: boolean;
  /** What protects saved secrets (Keychain, Windows encrypted storage), or null where tokens aren't kept. */
  tokenStore?: string | null;
  /** The plans a pasted token, or a saved one, can read. */
  listPlans?: (token: string, savedToken?: string) => Promise<YnabPlan[]>;
  listTokens?: () => Promise<SavedYnabToken[]>;
  forgetToken?: (id: string) => Promise<void>;
}) {
  const [tokens, setTokens] = useState<SavedYnabToken[]>([]);
  // The saved token in use, or null to paste one.
  const [savedId, setSavedId] = useState<string | null>(null);
  const choseToken = useRef(false);
  const [managing, setManaging] = useState(false);
  const [remember, setRemember] = useState(false);
  const [review, setReview] = useState<ImportResult | null>(null);
  const [differences, setDifferences] = useState<ImportResult | null>(null);
  const [failure, setFailure] = useState("");
  const [choices, setChoices] = useState<Record<string, string>>({});
  const [token, setToken] = useState("");
  const [planId, setPlanId] = useState(initialPlan ?? "");
  const [plans, setPlans] = useState<YnabPlan[] | null>(null);
  const [plansError, setPlansError] = useState("");
  const [loadingPlans, setLoadingPlans] = useState(false);
  const asked = useRef(0);
  const [busy, setBusy] = useState(false);
  const [message, setMessage] = useState("");
  const [replace, setReplace] = useState(false);
  const secret = token.trim();
  const saved = tokens.find((t) => t.id === savedId) ?? null;
  const planName = plans?.find((p) => p.id === planId)?.name ?? t("my plan");
  // Saved tokens, the most recently used one picked unless a token's already being pasted.
  const loadTokens = async (pick?: string) => {
    if (!tokenStore) return;
    try {
      const found = await listTokens();
      setTokens(found);
      setSavedId((id) => pick ?? (found.some((t) => t.id === id) ? id : null));
      return found;
    } catch {
      setTokens([]);
    }
  };
  useEffect(() => {
    void loadTokens().then((found) => {
      if (found?.length && !choseToken.current) setSavedId((id) => id ?? found[0].id);
    });
    // Only once: later loads come from what the form does.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);
  // Lists the token's plans shortly after it's pasted (at once for a saved one), so the plan is
  // picked by name. Keeps the linked plan, or the one already picked, when the token reads it.
  useEffect(() => {
    // Invalidate the old request as soon as the credential changes, including while the new
    // token is empty or still in its debounce delay.
    const n = ++asked.current;
    if (!secret && !savedId) {
      setLoadingPlans(false);
      return;
    }
    setLoadingPlans(true);
    const wait = setTimeout(async () => {
      try {
        const found = await listPlans(savedId ? "" : secret, savedId ?? undefined);
        if (n !== asked.current) return;
        setPlans(found);
        setPlansError("");
        setPlanId((id) =>
          found.some((p) => p.id === id) ? id : (found.find((p) => p.id === initialPlan) ?? found[0])?.id ?? "",
        );
      } catch (e) {
        if (n !== asked.current) return;
        setPlans(null);
        setPlanId(initialPlan ?? "");
        setPlansError(String(e));
      } finally {
        if (n === asked.current) setLoadingPlans(false);
      }
    }, savedId ? 0 : 400);
    return () => {
      clearTimeout(wait);
      if (n === asked.current) asked.current++;
    };
  }, [secret, savedId, listPlans, initialPlan]);
  const shownName = (p: YnabPlan) =>
    p.lastModifiedOn && plans!.filter((q) => q.name === p.name).length > 1
      ? t("{name} · edited {date}", { name: p.name, date: formatDate(p.lastModifiedOn.slice(0, 10)) })
      : p.name;
  // A saved token is named for the plans it reads. The name is stored with the token, so it
  // stays in English like other stored names.
  const tokenLabel = () => {
    const names = (plans ?? []).map((p) => p.name);
    return names.length > 3 ? `${names.slice(0, 3).join(", ")} +${names.length - 3} more` : names.join(", ");
  };
  const done = (r: ImportResult) => {
    setReview(null);
    setDifferences(null);
    setFailure("");
    const seconds = (r.elapsedMs / 1000).toFixed(2);
    const values = {
      name: r.snapshot.budget.name,
      accounts: tn(r.snapshot.budget.accounts.length, "{count} accounts", "{count} accounts"),
      transactions: tn(r.snapshot.budget.transactions.length, "{count} transactions", "{count} transactions"),
      seconds,
    };
    setMessage(
      r.unchanged
        ? t("Already up to date · {seconds} seconds.", { seconds })
        : r.incremental
          ? t(
              "Imported and saved {name} · {accounts} · {transactions} · {seconds} seconds. Downloaded changes only. A backup was saved automatically.",
              values,
            )
          : t(
              "Imported and saved {name} · {accounts} · {transactions} · {seconds} seconds. A backup was saved automatically.",
              values,
            ),
    );
  };
  // An import waits on a choice for each competing change, or on its differences from YNAB.
  const settle = (r: ImportResult) => {
    if (r.reviewId && r.differences?.length) {
      setDifferences(r);
      setMessage("");
      return;
    }
    if (r.reviewId) {
      setDifferences(null);
      setReview(r);
      setChoices({});
      setMessage(t("Review competing changes below. Nothing has been overwritten."));
      return;
    }
    done(r);
  };
  const run = async (work: () => Promise<void>) => {
    setBusy(true);
    try {
      await work();
    } catch (err) {
      setMessage("");
      setFailure(String(err));
    } finally {
      setBusy(false);
    }
  };
  const acceptDifferences = (ask: boolean) =>
    run(async () => {
      const pending = differences!;
      const r = await onAccept!(pending.reviewId!);
      settle(r);
      if (ask && !r.reviewId) onAsk?.(ynabHelpText(r.snapshot.budget.name, { differences: pending.differences ?? [] }));
    });
  return (
    <section className="panel ynab-import">
      <h2>{t("Import from YNAB")}</h2>
      <p className="import-lead">
        {t("Brings over your accounts, categories, transactions, targets, notes and scheduled transactions, and checks every total against YNAB. This budget is backed up first, and UrBudget never changes anything in YNAB.")}
      </p>
      {message && (
        <p className="import-result" role="status">
          {message}
        </p>
      )}
      {failure && (
        <div className="ynab-problem" role="alert">
          <TriangleAlert size={16} aria-hidden />
          <div>
            <strong>{t("The import stopped. Nothing was changed.")}</strong>
            <p>{failure}</p>
            <div className="form-actions">
              {onAsk && (
                <button
                  type="button"
                  className="secondary small"
                  onClick={() => onAsk(ynabHelpText(planName, { error: failure }))}
                >
                  <MessageSquareText size={14} /> {t("Ask the assistant")}
                </button>
              )}
              <button type="button" className="text-button" onClick={() => setFailure("")}>
                {t("Dismiss")}
              </button>
            </div>
          </div>
        </div>
      )}
      {differences && (
        <div className="ynab-problem ynab-differences" role="alert">
          <TriangleAlert size={16} aria-hidden />
          <div>
            <strong>
              {differences.differences!.length === 1
                ? t("One total doesn't match YNAB")
                : tn(differences.differences!.length, "{count} totals don't match YNAB", "{count} totals don't match YNAB")}
            </strong>
            <p>
              {t("Nothing has been saved yet. UrBudget rebuilt {plan} from YNAB's transactions, and these came out different from what YNAB shows:", { plan: planName })}
            </p>
            <ul>
              {differences.differences!.slice(0, SHOWN).map((d, i) => (
                <li key={i}>{differenceText(d)}</li>
              ))}
              {differences.differences!.length > SHOWN && (
                <li>{tn(differences.differences!.length - SHOWN, "and {count} more", "and {count} more")}</li>
              )}
            </ul>
            <p className="help">
              {t("Importing anyway brings everything over as YNAB has it, and keeps these listed under Settings › Backup & export. A later import asks again only if they change.")}
            </p>
            <div className="form-actions">
              <button type="button" className="text-button" disabled={busy} onClick={() => setDifferences(null)}>
                {t("Cancel")}
              </button>
              {onAsk && (
                <button type="button" className="secondary" disabled={busy || !onAccept} onClick={() => void acceptDifferences(true)}>
                  <MessageSquareText size={14} /> {t("Import and ask the assistant")}
                </button>
              )}
              <button type="button" className="primary" disabled={busy || !onAccept} onClick={() => void acceptDifferences(false)}>
                {t("Import anyway")}
              </button>
            </div>
          </div>
        </div>
      )}
      {review?.reviewId && (
        <div className="conflict-review">
          <h3>{t("Choose which changes to keep")}</h3>
          <p>{t("Independent edits are merged automatically. Only these competing changes need a choice.")}</p>
          <table>
            <thead>
              <tr>
                <th>{t("Change")}</th>
                <th>{t("Here")}</th>
                <th>{t("In YNAB")}</th>
                <th>{t("Keep")}</th>
              </tr>
            </thead>
            <tbody>
              {review.conflicts?.map((c) => (
                <tr key={c.path}>
                  <td>
                    <span className="conflict-about">{c.about ?? c.path}</span>
                    {c.field && (
                      <small className="conflict-field">
                        {CONFLICT_FIELDS[c.field] ? t(CONFLICT_FIELDS[c.field]) : c.field}
                      </small>
                    )}
                  </td>
                  <td>
                    <pre>{conflictValue(c.local, c.field, c.localText)}</pre>
                  </td>
                  <td>
                    <pre>{conflictValue(c.remote, c.field, c.remoteText)}</pre>
                  </td>
                  <td>
                    <select
                      aria-label={t("Resolve {path}", { path: c.path })}
                      value={choices[c.path] ?? ""}
                      onChange={(e) =>
                        setChoices({ ...choices, [c.path]: e.target.value })
                      }
                    >
                      <option value="">{t("Choose…")}</option>
                      <option value="local">{t("Keep this budget's")}</option>
                      <option value="remote">{t("Take YNAB's")}</option>
                    </select>
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
          <button
            className="primary"
            disabled={
              busy ||
              !onResolve ||
              review.conflicts?.some((c) => !choices[c.path])
            }
            onClick={() =>
              void run(async () => {
                await onResolve!(review.reviewId!, choices);
                setReview(null);
                setMessage(t("Reviewed changes imported and saved. A backup was created."));
              })
            }
          >
            {t("Apply reviewed import")}
          </button>
          <button className="text-button" onClick={() => setReview(null)}>
            {t("Cancel review")}
          </button>
        </div>
      )}
      <form
        className="ynab-form"
        onSubmit={(e) => {
          e.preventDefault();
          setMessage(t("Importing…"));
          setFailure("");
          setDifferences(null);
          const pasted = secret;
          const label = tokenLabel();
          setToken("");
          void run(async () => {
            const r = await onImport(
              savedId
                ? { token: "", planId, replace, savedToken: savedId, ...(label ? { tokenLabel: label } : {}) }
                : { token: pasted, planId, replace, ...(remember && tokenStore ? { remember: true, tokenLabel: label } : {}) },
            );
            if (r.savedToken) {
              setRemember(false);
              await loadTokens(r.savedToken);
            } else if (savedId) {
              await loadTokens();
            }
            settle(r);
          });
        }}
      >
        <div className="ynab-field">
          <span className="ynab-label" id="ynab-token-label">{t("YNAB token")}</span>
          {saved ? (
            <div className="saved-token" aria-labelledby="ynab-token-label">
              <KeyRound size={15} aria-hidden />
              <span className="saved-token-name">{saved.label}</span>
              <span className="saved-token-hint">{ltr(`••••${saved.hint}`)}</span>
              <button type="button" className="text-button" onClick={() => setManaging(true)}>
                {tc("command", "Change")}
              </button>
            </div>
          ) : (
            <>
              <input
                type="password"
                aria-label={t("YNAB personal access token")}
                placeholder={t("Paste your personal access token")}
                autoComplete="off"
                spellCheck={false}
                required
                value={token}
                onChange={(e) => {
                  choseToken.current = true;
                  setToken(e.target.value);
                }}
              />
              <div className="token-options">
                {tokenStore && (
                  <label className="checkbox-label">
                    <input type="checkbox" checked={remember} onChange={(e) => setRemember(e.target.checked)} />
                    {t("Remember on this device")}
                  </label>
                )}
                {tokens.length > 0 && (
                  <button type="button" className="text-button" onClick={() => setManaging(true)}>
                    {t("Use a saved token")}
                  </button>
                )}
              </div>
              <details className="token-help">
                <summary>{t("Where do I get a token?")}</summary>
                <p>
                  {tokenStore
                    ? t(
                        "In YNAB on the web, open Account Settings, then Developer Settings, and choose New Token. Copy it right away: YNAB shows it only once. A remembered token is kept in {store}; otherwise it's used only for this import.",
                        { store: t(tokenStore) },
                      )
                    : t(
                        "In YNAB on the web, open Account Settings, then Developer Settings, and choose New Token. Copy it right away: YNAB shows it only once. It's used only for this import and never stored.",
                      )}
                </p>
              </details>
            </>
          )}
        </div>
        <label className="ynab-field">
          <span className="ynab-label">{t("Plan")}</span>
          <select
            required
            disabled={!plans || loadingPlans}
            value={planId}
            onChange={(e) => setPlanId(e.target.value)}
          >
            {plans ? (
              plans.map((p) => (
                <option key={p.id} value={p.id}>
                  {shownName(p)}
                </option>
              ))
            ) : (
              <option value={planId}>
                {loadingPlans
                  ? t("Loading your plans…")
                  : plansError
                    ? t("No plans to show")
                    : t("Paste your token to choose a plan")}
              </option>
            )}
          </select>
        </label>
        {plansError && <p className="help">{plansError}</p>}
        {!blank && (
        <fieldset className="ynab-mode">
          <legend>{t("If this budget already has data")}</legend>
          <label className={`ynab-choice${replace ? "" : " selected"}`}>
            <input type="radio" name="ynab-mode" checked={!replace} onChange={() => setReplace(false)} />
            <span>
              {t("Merge with this budget")}
              <small>{t("Brings in YNAB's changes and keeps edits made here. Competing changes are shown for review.")}</small>
            </span>
          </label>
          <label className={`ynab-choice${replace ? " selected" : ""}`}>
            <input type="radio" name="ynab-mode" checked={replace} onChange={() => setReplace(true)} />
            <span>
              {t("Replace this budget")}
              <small>{t("Starts over from YNAB's version. The current budget stays in a backup.")}</small>
            </span>
          </label>
        </fieldset>
        )}
        <div className="form-actions">
          <button className="primary" disabled={busy || loadingPlans || !planId} type="submit">
            {busy ? t("Importing…") : t("Import & save")}
          </button>
        </div>
      </form>
      {managing && tokenStore && (
        <SavedTokens
          tokens={tokens}
          current={savedId}
          store={tokenStore}
          close={() => setManaging(false)}
          use={(id) => {
            choseToken.current = true;
            setSavedId(id);
            setManaging(false);
          }}
          paste={() => {
            choseToken.current = true;
            setSavedId(null);
            setPlans(null);
            setPlanId(initialPlan ?? "");
            setManaging(false);
          }}
          forget={async (id) => {
            await forgetToken(id);
            const left = (await loadTokens()) ?? [];
            if (id === savedId) {
              setSavedId(left[0]?.id ?? null);
              if (!left.length) setPlans(null);
            }
          }}
        />
      )}
    </section>
  );
}

/** The YNAB tokens kept on this device: use one, or delete it. */
function SavedTokens({
  tokens,
  current,
  store,
  close,
  use,
  paste,
  forget,
}: {
  tokens: SavedYnabToken[];
  current: string | null;
  store: string;
  close: () => void;
  use: (id: string) => void;
  paste: () => void;
  forget: (id: string) => Promise<void>;
}) {
  const [confirming, setConfirming] = useState<string | null>(null);
  const [error, setError] = useState("");
  const day = (at: string) => formatDate(at.slice(0, 10));
  return (
    <Modal title={t("Saved YNAB tokens")} close={close} className="saved-tokens">
      <p className="help">
        {t("Kept in {store} on this device. Deleting one here doesn't switch it off in YNAB; to do that, delete it under Developer Settings in YNAB.", { store: t(store) })}
      </p>
      {error && <p className="danger-text" role="alert">{error}</p>}
      {tokens.length ? (
        <ul className="token-list">
          {tokens.map((token) => (
            <li key={token.id} className={token.id === current ? "current" : undefined}>
              <KeyRound size={16} aria-hidden />
              <span className="token-text">
                <strong>{token.label}</strong>
                <small>
                  {token.lastUsed
                    ? t("••••{hint} · saved {saved} · last used {used}", {
                        hint: token.hint,
                        saved: day(token.savedAt),
                        used: day(token.lastUsed),
                      })
                    : t("••••{hint} · saved {saved}", { hint: token.hint, saved: day(token.savedAt) })}
                </small>
              </span>
              {confirming === token.id ? (
                <span className="token-actions">
                  <button
                    type="button"
                    className="danger small"
                    onClick={async () => {
                      try {
                        await forget(token.id);
                        setConfirming(null);
                        setError("");
                      } catch (e) {
                        setError(String(e));
                      }
                    }}
                  >
                    {t("Delete")}
                  </button>
                  <button type="button" className="text-button" onClick={() => setConfirming(null)}>
                    {t("Keep")}
                  </button>
                </span>
              ) : (
                <span className="token-actions">
                  {token.id === current ? (
                    <span className="token-current">{t("In use")}</span>
                  ) : (
                    <button type="button" className="secondary small" onClick={() => use(token.id)}>
                      {t("Use")}
                    </button>
                  )}
                  <button
                    type="button"
                    className="icon-button"
                    aria-label={t("Delete {name} (••••{hint})", { name: token.label, hint: token.hint })}
                    title={t("Delete")}
                    onClick={() => setConfirming(token.id)}
                  >
                    <Trash2 size={15} />
                  </button>
                </span>
              )}
            </li>
          ))}
        </ul>
      ) : (
        <p className="empty">{t("No saved tokens. Paste one and choose Remember on this device.")}</p>
      )}
      <div className="form-actions">
        <button type="button" className="secondary" onClick={paste}>
          {t("Paste a different token")}
        </button>
        <button type="button" className="primary" onClick={close}>
          {t("Done")}
        </button>
      </div>
    </Modal>
  );
}
