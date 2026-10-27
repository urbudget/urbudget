import { useMemo, useRef, useState, type ReactNode } from "react";
import { MessageSquareText, Undo2 } from "lucide-react";
import { api, type BankRow, type RefundOf } from "./bridge";
import { placeholder } from "./bankNames";
import { formatDate, money } from "./format";
import { today, type Budget } from "./types";
import { useVirtualRows } from "./ui";
import "./import.css";

/** Each review row is this tall, so a long bank file only renders the rows in view. */
const REVIEW_ROW = 41;
/**
 * A statement's text: UTF-16 or UTF-8 as its byte-order mark says, else UTF-8, else what the engine
 * makes of its bytes: a workbook's sheet as CSV (the one named, else the one most like a statement,
 * with the names of the others), a ZIP of statements, or text in the encoding its bytes fit
 * (Windows-1250 or -1252 in Europe, Shift_JIS, GBK, Windows-1251…). A PDF it turns away.
 */
export async function statementFile(file: Blob, sheet: string | null = null): Promise<{ text: string; sheets: string[]; sheet?: string }> {
  const bytes = new Uint8Array(await file.arrayBuffer());
  const text = (t: string) => ({ text: t, sheets: [] });
  const decode = (encoding: string, from = 0) => text(new TextDecoder(encoding).decode(bytes.subarray(from)));
  // The engine gets the bytes as ISO-8859-1, a character for each.
  const engine = async () => {
    let latin1 = "";
    for (let i = 0; i < bytes.length; i += 0x8000) latin1 += String.fromCharCode(...bytes.subarray(i, i + 0x8000));
    const read = await api.decodeStatement(latin1, sheet);
    return { text: read.text, sheets: read.sheets ?? [], sheet: read.sheet };
  };
  // An Excel or OpenDocument workbook, a ZIP, a PDF: not text at all.
  const starts = (...signature: number[]) => signature.every((b, i) => bytes[i] === b);
  if (starts(0x50, 0x4b, 0x03, 0x04) || starts(0xd0, 0xcf, 0x11, 0xe0) || starts(0x25, 0x50, 0x44, 0x46)) return engine();
  if (bytes[0] === 0xff && bytes[1] === 0xfe) return decode("utf-16le", 2);
  if (bytes[0] === 0xfe && bytes[1] === 0xff) return decode("utf-16be", 2);
  // UTF-16 without a mark: plain text has a zero in every other byte.
  const head = bytes.subarray(0, 400);
  const zeros = (odd: number) => head.filter((b, i) => i % 2 === odd && b === 0).length;
  if (head.length >= 4 && zeros(1) > head.length / 4) return decode("utf-16le");
  if (head.length >= 4 && zeros(0) > head.length / 4) return decode("utf-16be");
  try {
    return text(new TextDecoder("utf-8", { fatal: true }).decode(bytes));
  } catch {
    return engine();
  }
}
/** A statement's text (see `statementFile`). */
export const statementText = async (file: Blob) => (await statementFile(file)).text;

/**
 * A row's choice lists at most this many possible matches, the nearest in date: an account with
 * thousands of the same charge would otherwise give every row thousands of options to draw.
 */
const LISTED = 50;
/** A CSV to start from: the headers the importer reads and a few example rows. */
export function bankTemplate(dateOrder = "ymd", month = today().slice(0, 7)) {
  const [y, m] = month.split("-");
  const date = (d: string) => (dateOrder === "dmy" ? `${d}/${m}/${y}` : dateOrder === "mdy" ? `${m}/${d}/${y}` : `${y}-${m}-${d}`);
  return [
    "Date,Payee,Memo,Outflow,Inflow",
    `${date("01")},Employer,Salary,,5000.00`,
    `${date("03")},Grocery store,Weekly shop,85.40,`,
    `${date("05")},Electric company,Monthly bill,120.00,`,
    "",
  ].join("\r\n");
}

/** What the assistant is asked when someone wants help with a file: ready to send, not sent. */
export function importHelpText(account: string, problem = "") {
  return [
    `Help me import this file into ${account}.`,
    problem && `The import preview said: ${problem}`,
    "Find the transactions, ask me about anything unclear, and prepare them for the import review.",
  ]
    .filter(Boolean)
    .join(" ");
}

/** What the assistant is asked about the lines of an import that need attention: ready to send, not sent. */
export const fixImportText =
  "Fix the lines of this import that need attention. Use only what the file itself supports, and ask me about anything that isn't clear.";

/** New transactions an import left without a category, and the accounts they went into. */
export type Uncategorized = { ids: string[]; accounts: string[] };
/** The assistant reads at most this many attached transactions at a time. */
export const ATTACH_LIMIT = 200;

/**
 * The transactions in `after` that weren't in `before` and still have no category: none at all,
 * or the Uncategorized placeholder. Payee rules may have categorized the rest. Tracking accounts
 * don't use categories, and transfers don't need one.
 */
export function uncategorizedImports(before: Budget, after: Budget): Uncategorized {
  const known = new Set(before.transactions.map((t) => t.id));
  const budgeted = new Set(after.accounts.filter((a) => a.kind !== "tracking" && a.onBudget !== false).map((a) => a.id));
  const fresh = after.transactions.filter(
    (t) =>
      !known.has(t.id) &&
      budgeted.has(t.accountId) &&
      !t.transferAccountId &&
      t.splits.every((s) => !s.transferAccountId && (!s.categoryId || placeholder(after, s.categoryId))),
  );
  const accounts = [...new Set(fresh.map((t) => t.accountId))].map((id) => after.accounts.find((a) => a.id === id)!.name);
  return { ids: fresh.map((t) => t.id), accounts };
}
const join = (names: string[]) =>
  names.length < 2 ? names.join("") : `${names.slice(0, -1).join(", ")} and ${names[names.length - 1]}`;
/** What the assistant is asked about them, ready to send with them attached; never sent by itself. */
export function categorizeText({ ids, accounts }: Uncategorized) {
  const where = join(accounts);
  if (ids.length > ATTACH_LIMIT) return `Categorize these ${ATTACH_LIMIT} of the ${ids.length} new transactions in ${where}.`;
  return ids.length === 1
    ? `Categorize the new transaction in ${where}.`
    : `Categorize the ${ids.length} new transactions in ${where}.`;
}

export type ImportChoice = { row: BankRow; action: string; matchId: string | null };
/** What's chosen in a review so far: each row's choice, and the ID each row added separately imports with. */
export type ReviewChoices = { picked: Record<string, string>; separate?: Record<string, string> };

/**
 * Skip known bank IDs and take a single match only when that account side has not been imported.
 * A transfer the bank text doesn't back up is only suggested: amount and date alone can't tell it
 * from a card payment of the same sum.
 */
const defaultChoice = (r: BankRow) => {
  if (r.duplicate) return "skip";
  if (r.possibleDuplicate) return "review";
  if (!r.candidates.length) return "new";
  const [only] = r.candidates;
  if (r.candidates.length > 1 || r.previouslyMatchedCandidates?.includes(only)) return "review";
  return r.transferCandidates?.includes(only) && !r.transferSure ? "review" : `match:${only}`;
};

/** What a row of money in may be: a refund of a purchase, with its category when it gives one. */
export function refundText(refund: RefundOf, budget: Budget) {
  if (refund.purchases > 1 && !refund.categoryId) return `Possible refund · ${refund.purchases} purchases from ${refund.payee} match`;
  const category = refund.categoryId && budget.categories.find((c) => c.id === refund.categoryId)?.name;
  return `Refund of ${refund.payee} · ${money(-refund.amount)} on ${formatDate(refund.date)}${category ? ` · ${category}` : ""}`;
}

/**
 * The review of transactions bound for one account, from a bank file or from bank sync: what
 * each row does (add it, match an existing transaction, or skip it), then one import.
 */
export function ImportReview({
  budget,
  rows,
  busy,
  icon,
  title,
  action,
  cancelLabel = "Cancel",
  onCancel,
  onImport,
  choices: given,
  onChoices,
}: {
  budget: Budget;
  rows: BankRow[];
  busy: boolean;
  icon: ReactNode;
  /** Where the rows come from and where they go, unless the page says so already. */
  title?: ReactNode;
  /** A link beside the summary, such as one to choose another file. */
  action?: ReactNode;
  cancelLabel?: string;
  onCancel: () => void;
  /** Every row's choice, and how many rows are added or matched. */
  onImport: (choices: ImportChoice[], count: number) => void;
  /** The choices, kept by whoever shows the review when they must outlast it (a page that leaves it and comes back). */
  choices?: ReviewChoices;
  onChoices?: (next: ReviewChoices) => void;
}) {
  // Rows checked again (bank sync does when the budget changes) keep the choices made so far.
  const [own, setOwn] = useState<ReviewChoices>({ picked: {} });
  const kept = given ?? own;
  const keep = onChoices ?? setOwn;
  const choices = kept.picked;
  const setChoices = (picked: Record<string, string>) => keep({ ...kept, picked });
  // Kept choices are checked against the rows as they are now, as any other.
  const choice = (r: BankRow) => {
    if (r.duplicate) return r.idless && choices[r.id] === "separate" ? "separate" : "skip";
    const selected = choices[r.id];
    if (selected?.startsWith("match:") && !r.candidates.includes(selected.slice(6))) return "review";
    return selected ?? defaultChoice(r);
  };
  const review = useRef<HTMLDivElement>(null);
  const inView = useVirtualRows(review, rows.length, REVIEW_ROW);
  const byId = useMemo(() => new Map(budget.transactions.map((t) => [t.id, t])), [budget.transactions]);
  const accountNames = useMemo(() => new Map(budget.accounts.map((a) => [a.id, a.name])), [budget.accounts]);
  // The nearest possible matches by date, worked out once for each list and day (rows share lists).
  const nearest = useMemo(() => new WeakMap<string[], Map<string, string[]>>(), [byId]);
  const listed = (r: BankRow) => {
    if (r.candidates.length <= LISTED) return r.candidates;
    let byDay = nearest.get(r.candidates);
    if (!byDay) nearest.set(r.candidates, (byDay = new Map()));
    let list = byDay.get(r.date);
    if (!list) {
      const legs = new Map(r.candidateDetails?.map((c) => [c.id, c.transactionId]));
      const day = Date.parse(r.date);
      const apart = (id: string) => {
        const t = byId.get(legs.get(id) ?? id);
        return t ? Math.abs(Date.parse(t.date) - day) : Infinity;
      };
      list = r.candidates
        .map((id) => [apart(id), id] as const)
        .sort((a, b) => a[0] - b[0])
        .slice(0, LISTED)
        .map(([, id]) => id);
      byDay.set(r.date, list);
    }
    const chosen = choice(r).startsWith("match:") ? choice(r).slice(6) : null;
    return chosen && !list.includes(chosen) ? [...list, chosen] : list;
  };
  const transfer = (r: BankRow, c: string) => c.startsWith("match:") && !!r.transferCandidates?.includes(c.slice(6));
  const importing = rows.filter((r) => choice(r) !== "skip").length;
  const transfers = rows.filter((r) => transfer(r, choice(r))).length;
  // Every row still to decide, the same way.
  const decide = (to: string) =>
    setChoices({ ...choices, ...Object.fromEntries(rows.filter((r) => choice(r) === "review").map((r) => [r.id, to])) });
  // Rows still to decide whose transfer is only suggested.
  const suggested = rows.filter((r) => choice(r) === "review" && r.transferCandidates?.length);
  const undecided = rows.filter((r) => choice(r) === "review").length;
  const duplicates = rows.filter((r) => r.duplicate).length;
  const renamed = rows.filter((r) => !r.duplicate && r.rename).length;
  const refunds = rows.filter((r) => !r.duplicate && r.refundOf).length;
  // A row added separately keeps its ID with the choices, so trying the import again (after a save
  // that failed once the budget had it) can't add it twice, even from a review shown again.
  const chosen = () => {
    const separate = { ...kept.separate };
    const out = rows.map((row) => {
      const c = choice(row);
      if (c === "separate") {
        const importId = (separate[row.id] ??= `bank:manual:${crypto.randomUUID()}`);
        return { row: { ...row, importId }, action: "new", matchId: null };
      }
      return { row, action: c.startsWith("match:") ? "match" : c, matchId: c.startsWith("match:") ? c.slice(6) : null };
    });
    if (Object.keys(separate).length !== Object.keys(kept.separate ?? {}).length) keep({ ...kept, separate });
    return out;
  };
  return (
    <>
      <div className="import-summary">
        {icon}
        <span>
          {title && <>{title} · </>}
          {rows.length} {rows.length === 1 ? "row" : "rows"}
          {duplicates > 0 && ` · ${duplicates} already imported`}
          {renamed > 0 && ` · ${renamed} renamed by your payee rules`}
          {refunds > 0 && ` · ${refunds} ${refunds === 1 ? "refund" : "refunds"}`}
          {transfers > 0 && ` · ${transfers} ${transfers === 1 ? "transfer" : "transfers"} between your accounts`}
        </span>
        {action}
      </div>
      <p className="help import-note">
        Matching keeps your category and memo and marks the transaction cleared. Imported transactions wait for your
        approval.
      </p>
      {rows.some((r) => r.transferCandidates?.length) && (
        <p className="help import-note">
          A transfer joins a row to the uncategorized entry it mirrors in another account, so the money you moved isn't
          counted as spending or income. It's chosen for you when the bank's text names the other account or matches
          earlier transfers between them; otherwise it's suggested for you to check.
        </p>
      )}
      {rows.some((r) => r.possibleDuplicate || (r.idless && r.duplicate)) && <p className="help import-note">
        This file has no bank transaction IDs. A row may already be imported; compare its memo and amount.
        Skip it if it is the same payment, or add it separately if it is a different payment.
      </p>}
      <div className="table-scroll import-review" ref={review}>
        <table>
          <thead>
            <tr>
              <th>Date</th>
              <th>Payee</th>
              <th className="num">Amount</th>
              <th>What to do</th>
            </tr>
          </thead>
          <tbody>
            {inView.before > 0 && (
              <tr aria-hidden className="spacer-row" style={{ height: inView.before }}>
                <td colSpan={4} />
              </tr>
            )}
            {rows.slice(inView.start, inView.end).map((r, i) => {
              const index = inView.start + i;
              return (
                <tr key={`${r.id}:${index}`} className={r.duplicate ? "muted" : ""}>
                  <td>{formatDate(r.date)}</td>
                  <td title={r.rename ? `${r.rename}, from the bank's “${r.payee}”` : undefined}>
                    {r.rename ?? r.payee}
                    {r.rename && <small className="bank-text">{r.payee}</small>}
                    {r.memo && <small className="bank-text" title={r.memo}>{r.memo}</small>}
                    {r.refundOf && !r.duplicate && (
                      <small className="refund-note">
                        <Undo2 size={11} aria-hidden /> {refundText(r.refundOf, budget)}
                      </small>
                    )}
                  </td>
                  <td className="num">{money(r.amount)}</td>
                  <td>
                    {r.duplicate && !r.idless ? (
                      "Already imported · skipped"
                    ) : (
                      <select
                        aria-label={`Import action ${index + 1}`}
                        disabled={busy}
                        value={choice(r)}
                        onChange={(e) => setChoices({ ...choices, [r.id]: e.target.value })}
                      >
                        {!r.duplicate && (
                          <option value="review">
                            {r.possibleDuplicate
                              ? "May already be imported — choose"
                              : r.transferCandidates?.length
                                ? "Transfer suggested — check it and choose"
                                : "Choose a match or add it"}
                          </option>
                        )}
                        {r.duplicate
                          ? <option value="separate">Add as a separate transaction</option>
                          : <option value="new">Add as a new transaction</option>}
                        <option value="skip">{r.duplicate ? "Already imported — skip" : "Skip"}</option>
                        {(r.duplicate ? [] : listed(r)).map((id) => {
                          const detail = r.candidateDetails?.find((c) => c.id === id);
                          const t = byId.get(detail?.transactionId ?? id);
                          const part = detail && t?.splits[detail.splitIndex];
                          const other = r.transferCandidates?.includes(id) && t && accountNames.get(t.accountId);
                          return (
                            <option key={id} value={`match:${id}`}>
                              {other ? `Transfer ${r.amount > 0 ? "from" : "to"} ${other}: ` : "Match: "}
                              {t ? formatDate(t.date) : ""} · {t?.payee || "Transfer"}
                              {detail ? ` · Split ${detail.splitIndex + 1}` : ""}
                              {part?.memo || t?.memo ? ` · ${part?.memo || t?.memo}` : ""}
                              {r.previouslyMatchedCandidates?.includes(id) ? " · Previously imported" : ""}
                            </option>
                          );
                        })}
                        {!r.duplicate && r.candidates.length > LISTED && (
                          <option disabled>
                            {r.candidates.length - LISTED} more of the same amount within ten days aren't listed
                          </option>
                        )}
                      </select>
                    )}
                  </td>
                </tr>
              );
            })}
            {inView.after > 0 && (
              <tr aria-hidden className="spacer-row" style={{ height: inView.after }}>
                <td colSpan={4} />
              </tr>
            )}
          </tbody>
        </table>
      </div>
      <div className="form-actions">
        {undecided > 0 && (
          <span className="help">
            {undecided} {undecided === 1 ? "row needs" : "rows need"} a choice
          </span>
        )}
        {suggested.length > 0 && (
          <button
            className="secondary"
            disabled={busy}
            onClick={() =>
              setChoices({ ...choices, ...Object.fromEntries(suggested.map((r) => [r.id, `match:${r.transferCandidates![0]}`])) })
            }
          >
            Pair {suggested.length} suggested {suggested.length === 1 ? "transfer" : "transfers"}
          </button>
        )}
        {undecided > 0 && (
          <>
            <button className="secondary" disabled={busy} onClick={() => decide("new")}>
              Add {undecided === 1 ? "it" : "them"} as new
            </button>
            <button className="secondary" disabled={busy} onClick={() => decide("skip")}>
              Skip {undecided === 1 ? "it" : "them"}
            </button>
          </>
        )}
        {/* Not while the import saves: it lands all the same. */}
        <button className="secondary" disabled={busy} onClick={onCancel}>
          {cancelLabel}
        </button>
        <button className="primary" disabled={busy || undecided > 0 || !importing} onClick={() => onImport(chosen(), importing)}>
          {busy ? "Importing…" : `Import ${importing} ${importing === 1 ? "transaction" : "transactions"}`}
        </button>
      </div>
    </>
  );
}

/** After an import: the new transactions without a category, offered to the assistant. */
export function CategorizeOffer({ uncategorized, onCategorize }: { uncategorized: Uncategorized; onCategorize: () => void }) {
  const n = uncategorized.ids.length;
  return (
    <div className="import-assist">
      <MessageSquareText size={16} aria-hidden />
      <p>
        {n === 1 ? "1 new transaction has no category." : `${n} new transactions have no category.`}
        <small>Opens the assistant with them attached. Nothing is sent until you press Send.</small>
      </p>
      <button type="button" className="secondary small" onClick={onCategorize}>
        Categorize with the assistant
      </button>
    </div>
  );
}
