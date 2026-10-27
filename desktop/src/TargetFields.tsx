import { today } from "./types";
import { DateInput } from "./ui";
import { msg, t, tn } from "./i18n";

const WEEKDAYS = [msg("Sunday"), msg("Monday"), msg("Tuesday"), msg("Wednesday"), msg("Thursday"), msg("Friday"), msg("Saturday")];

export type TargetMonthContext = {
  /** The budget month being edited. Defaults to the current month for standalone forms. */
  month?: string;
  currentMonth?: string;
};
/** Preserve imported metadata, but never carry hidden timing fields into another behavior. */
export function updateTarget(goal: Record<string, unknown>, values: Record<string, unknown>) {
  const next = { ...goal, ...values };
  const clearDates = () => { delete next.goal_target_date; delete next.goal_target_month; };
  if (values.goal_type !== undefined && values.goal_type !== goal.goal_type) {
    delete next.goal_day;
    next.goal_cadence = values.goal_type === "NEED" ? 1 : 0;
    next.goal_cadence_frequency = 1;
    if (values.goal_type !== "TBD" || goal.goal_cadence === 2) clearDates();
  }
  if (values.goal_cadence !== undefined && values.goal_cadence !== goal.goal_cadence) {
    delete next.goal_day;
    next.goal_cadence_frequency = 1;
    if (values.goal_cadence === 2 || goal.goal_cadence === 2) clearDates();
  }
  if (values.goal_day !== undefined && next.goal_cadence === 2) clearDates();
  // A plain monthly target has no due-date field, and a hidden later date would spread its amount.
  const monthly = (g: Record<string, unknown>) =>
    Number(g.goal_cadence ?? 1) === 1 && Number(g.goal_cadence_frequency ?? 1) === 1;
  if (next.goal_type === "NEED" && monthly(next) && !monthly(goal)) clearDates();
  return next;
}
export function TargetFields({
  goal,
  onChange,
  currentMonth = today().slice(0, 7),
  month = currentMonth,
  paymentCategory = false,
}: {
  goal: Record<string, unknown>;
  onChange: (g: Record<string, unknown>) => void;
  paymentCategory?: boolean;
} & TargetMonthContext) {
  const change = (values: Record<string, unknown>) =>
    onChange(updateTarget(goal, values));
  const type = String(goal.goal_type ?? "NEED");
  const cadence = Number(goal.goal_cadence ?? 1);
  const behavior =
    type === "NEED"
      ? goal.goal_needs_whole_amount === false
        ? "refill"
        : "set"
      : type;
  return (
    <>
      <label>
        {t("Target behavior")}
        <select
          aria-label={t("Target behavior")}
          value={behavior}
          onChange={(e) => {
            const value = e.target.value;
            change({
              goal_type: value === "set" || value === "refill" ? "NEED" : value,
              goal_needs_whole_amount:
                value === "refill" ? false : value === "set" ? true : null,
            });
          }}
        >
          <option value="set">{t("Set aside each period")}</option>
          <option value="refill">{t("Refill up to each period")}</option>
          <option value="TB">{t("Maintain a category balance")}</option>
          <option value="TBD">{t("Reach a balance by a date")}</option>
          {type === "MF" && <option value="MF">{t("Monthly funding (imported target)")}</option>}
          {type === "DEBT" && <option value="DEBT">{t("Monthly debt payment")}</option>}
        </select>
      </label>
      {type === "NEED" && (
        <div className="form-grid">
          <label>
            {t("Repeats")}
            <select
              aria-label={t("Target cadence")}
              value={cadence}
              onChange={(e) => change({ goal_cadence: Number(e.target.value) })}
            >
              <option value={0}>{t("Once by a date")}</option>
              <option value={1}>{t("Monthly")}</option>
              <option value={2}>{t("Weekly")}</option>
              <option value={13}>{t("Yearly")}</option>
              {Array.from({ length: 10 }, (_, i) => (
                <option key={i} value={i + 3}>
                  {tn(i + 2, "Every {count} months", "Every {count} months")}
                </option>
              ))}
              <option value={14}>{t("Every two years")}</option>
            </select>
          </label>
          {[1, 2, 13].includes(cadence) && (
            <label>
              {t("Every (number of periods)")}
              <input
                type="number"
                aria-label={t("Target interval")}
                min={1}
                max={100}
                value={Number(goal.goal_cadence_frequency ?? 1)}
                onChange={(e) =>
                  change({ goal_cadence_frequency: Number(e.target.value) })
                }
              />
            </label>
          )}
        </div>
      )}
      {type === "NEED" && cadence === 2 ? (
        <label>
          {t("Day of week")}
          <select
            aria-label={t("Target weekday")}
            value={Number(goal.goal_day ?? 0)}
            onChange={(e) => change({ goal_day: Number(e.target.value) })}
          >
            {WEEKDAYS.map((d, i) => (
              <option key={d} value={i}>
                {t(d)}
              </option>
            ))}
          </select>
        </label>
      ) : (
        (type === "TBD" ||
          (type === "NEED" &&
            (cadence !== 1 ||
              Number(goal.goal_cadence_frequency ?? 1) > 1))) && (
          <label>
            {t("First due date")}
            <DateInput
              label={t("Target due date")}
              required
              value={String(goal.goal_target_date ?? goal.goal_target_month ?? "")}
              onChange={(iso) => change({ goal_target_date: iso })}
            />
          </label>
        )
      )}
      {type !== "TB" && (
        <label>
          {t("Start month")}
          <input
            aria-label={t("Target start month")}
            type="month"
            value={String(goal.goal_creation_month ?? today()).slice(0, 7)}
            onChange={(e) =>
              change({ goal_creation_month: `${e.target.value}-01` })
            }
          />
        </label>
      )}
      {month === currentMonth && !paymentCategory && (
        <label className="checkbox-label">
          <input
            aria-label={t("Snooze target this month")}
            type="checkbox"
            checked={String(goal.goal_snoozed_at ?? "").slice(0, 7) === currentMonth}
            onChange={(e) => change({ goal_snoozed_at: e.target.checked ? `${currentMonth}-01` : null })}
          />
          {t("Snooze target this month")}
        </label>
      )}
    </>
  );
}
