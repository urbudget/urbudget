"""What a new user would get from their bank texts, against the budget they keep by hand.

Reads sms-live-primary-data.json, written on the phone by SmsLiveCheck's "primary" part: Primary's
accounts and transactions, and those of a budget set up from nothing from the same bank texts, every
suggestion taken. Prints how the accounts line up, which transactions match (same account and
amount, a day apart at most), which Primary has that no text gave, which the texts gave that Primary
doesn't have, how transfers pair, how often payees and categories agree, and each account's balance
at the end of the period in both.

    python compare_onboarding.py sms-live-primary-data.json [--examples 15]
"""
import argparse
import json
import re
from collections import defaultdict
from datetime import date, timedelta


def digits(name):
    """The runs of three or more digits in an account's name ("ANB Checking - 0091" has "0091")."""
    return re.findall(r"[0-9]{3,}", name or "")


def day(text):
    return date.fromisoformat(text)


def money(cents):
    return f"{cents / 100:,.2f}"


def sides(t):
    """Each account a transaction moves and by how much: its own, and a transfer's other side."""
    out = [(t["account"], t["amount"])]
    if t["other"]:
        out.append((t["other"], -t["amount"]))
    else:
        out += [(p["other"], -p["amount"]) for p in t["parts"] if p["other"]]
    return out


def balances(budget, through):
    """Each account's balance at the end of [through]."""
    out = {a["id"]: a["opening"] for a in budget["accounts"]}
    for t in budget["transactions"]:
        if t["date"] <= through:
            for account, amount in sides(t):
                out[account] = out.get(account, 0) + amount
    return out


def main():
    args = argparse.ArgumentParser()
    args.add_argument("data")
    args.add_argument("--examples", type=int, default=15)
    a = args.parse_args()
    data = json.load(open(a.data, encoding="utf-8-sig"))
    start, end = data["from"], data["to"]
    primary, made = data["primary"], data["onboarding"]
    p_accounts = {x["id"]: x for x in primary["accounts"]}
    m_accounts = {x["id"]: x for x in made["accounts"]}
    print(f"Period {start} to {end}; {len(made['transactions'])} transactions from the texts, "
          f"{sum(1 for t in primary['transactions'] if start <= t['date'] <= end)} in Primary\n")

    # Accounts: each one made, to Primary's whose name carries its digits.
    print("Setup:")
    for c in data["setup"]:
        plan = c["plan"]
        print(f"  {c['sender']} {c['ref'] or '(no card)'}: {c['texts']} texts -> {plan['action']} {plan['name']} ({plan['type']})"
              + (f" into {plan['joins']}" if plan["action"] == "join" else ""))
    to_primary = {}
    print("\nAccounts:")
    for m in made["accounts"]:
        mine = digits(m["name"])
        found = [p for p in primary["accounts"] if any(d.endswith(x[-4:]) or x.endswith(d[-4:]) for d in digits(p["name"]) for x in mine)]
        open_found = [p for p in found if not p["archived"]] or found
        if len(open_found) == 1:
            p = open_found[0]
            to_primary[m["id"]] = p["id"]
            same = "same type" if p["type"] == m["type"] else f"type {m['type']}, Primary's {p['type']}"
            print(f"  {m['name']} = {p['name']} ({same})")
        else:
            print(f"  {m['name']} ({m['type']}): " + ("no Primary account with its digits" if not found else "several: " + ", ".join(p["name"] for p in found)))
    used = defaultdict(list)
    for m, p in to_primary.items():
        used[p].append(m_accounts[m]["name"])
    for p, names in used.items():
        if len(names) > 1:
            print(f"  ! {p_accounts[p]['name']} was made {len(names)} times: {', '.join(names)}")
    texted = {t["account"] for t in primary["transactions"] if t["bank"] and start <= t["date"] <= end}
    for p in sorted(texted - set(to_primary.values())):
        print(f"  missing: {p_accounts[p]['name']} ({p_accounts[p]['type']}), which has bank-imported transactions in the period")

    # Transactions: each from the texts to one of Primary's in the same account, same amount, a day apart at most.
    # A text's transaction may be dated a few days before the first text.
    floor = (day(start) - timedelta(days=3)).isoformat()
    pool = defaultdict(list)
    for i, t in enumerate(primary["transactions"]):
        if t["date"] < floor:
            continue
        for account, amount in sides(t):
            pool[(account, amount)].append(i)
    taken = set()
    matched, extra = [], []
    for t in sorted(made["transactions"], key=lambda t: t["date"]):
        account = to_primary.get(t["account"])
        options = [i for i in pool.get((account, t["amount"]), []) if i not in taken and abs((day(primary["transactions"][i]["date"]) - day(t["date"])).days) <= 1]
        if not options:
            extra.append(t)
            continue
        i = min(options, key=lambda i: abs((day(primary["transactions"][i]["date"]) - day(t["date"])).days))
        taken.add(i)
        matched.append((t, primary["transactions"][i]))
    missed = [t for i, t in enumerate(primary["transactions"]) if i not in taken and start <= t["date"] <= end
              and (t["account"] in to_primary.values() or t["other"] in to_primary.values())]

    print(f"\nTransactions: {len(matched)} matched, {len(extra)} from the texts not in Primary, {len(missed)} in Primary's matched accounts not from the texts")
    late = sum(1 for m, p in matched if m["date"] != p["date"])
    print(f"  {late} of the matched a day apart")
    transfers = [(m, p) for m, p in matched if m["other"]]
    paired = sum(1 for m, p in transfers if to_primary.get(m["other"]) in ({p["other"], p["account"]} | {x["other"] for x in p["parts"]}))
    print(f"  transfers: {len(transfers)} matched, {paired} with the same other account as Primary's")
    as_transfer = sum(1 for m, p in matched if not m["other"] and (p["other"] or any(x["other"] for x in p["parts"])))
    print(f"  {as_transfer} matched spending or income that Primary has as a transfer")
    spending = [(m, p) for m, p in matched if not m["other"] and not p["other"]]
    payees = sum(1 for m, p in spending if m["payee"].strip().lower() == p["payee"].strip().lower())
    cats = sum(1 for m, p in spending if [x["category"] for x in m["parts"]] == [x["category"] for x in p["parts"]] and m["parts"])
    print(f"  of {len(spending)} matched spending and income: {payees} with Primary's payee, {cats} with Primary's category name")
    missed_bank = [t for t in missed if t["bank"]]
    print(f"  of those not from the texts, {len(missed_bank)} came from the bank in Primary (texts or files), {len(missed) - len(missed_bank)} were entered by hand")

    def show(title, rows, names):
        if not rows:
            return
        print(f"\n{title} (first {min(a.examples, len(rows))}):")
        for t in rows[:a.examples]:
            other = f" -> {names.get(t['other'], {}).get('name', '?')}" if t["other"] else ""
            print(f"  {t['date']} {names.get(t['account'], {}).get('name', '?')}{other} {money(t['amount'])} {t['payee'][:40]}")

    show("From the texts, not in Primary", extra, m_accounts)
    show("In Primary from the bank, not from the texts", missed_bank, p_accounts)

    # Balances at the end of the period, and how much each moved in it: a balance before the texts
    # that no text gives is the user's to enter, so the change is what the texts can be held to.
    print("\nBalances at", end, "(and the change over the period)")
    pb, mb = balances(primary, end), balances(made, end)
    before = balances(primary, (day(start) - timedelta(days=1)).isoformat())
    for m, p in to_primary.items():
        diff = mb.get(m, 0) - pb.get(p, 0)
        moved_m, moved_p = mb.get(m, 0) - m_accounts[m]["opening"], pb.get(p, 0) - before.get(p, 0)
        print(f"  {m_accounts[m]['name']}: {money(mb.get(m, 0))}; Primary's {p_accounts[p]['name']}: {money(pb.get(p, 0))}"
              + ("" if diff == 0 else f" ({'+' if diff > 0 else ''}{money(diff)})")
              + f"; moved {money(moved_m)} against Primary's {money(moved_p)}")
    print(f"\n{data.get('unclear', 0)} texts left unclear for the user")


if __name__ == "__main__":
    main()
