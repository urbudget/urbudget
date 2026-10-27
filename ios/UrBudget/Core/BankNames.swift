import Foundation

/// Payee rules are the bank names a payee goes by: a bank payee containing a rule's `match` is
/// the rule's payee. The rules run in the shared engine (workflows.rs); this mirrors the matching
/// as desktop/src/bankNames.ts does, to offer a rule after an imported transaction is renamed.
enum BankNames {
    /// Bank text as rules read it: lower case, with punctuation and runs of spaces as one space.
    static func normalize(_ text: String) -> String {
        var out = ""
        var gap = false
        for character in text.lowercased() {
            if character.isLetter || character.isNumber {
                if gap && !out.isEmpty { out.append(" ") }
                gap = false
                out.append(character)
            } else {
                gap = true
            }
        }
        return out
    }

    /// The rule for some bank text: of the rules whose text it contains (ignoring case and
    /// punctuation, from the start of a word), the longest. Rules of the same length go in list order.
    static func rule(_ rules: [JSON], for text: String) -> JSON? {
        let padded = " " + normalize(text)
        var found: JSON?
        var length = 0
        for rule in rules {
            let pattern = normalize(rule.text("match"))
            let size = pattern.unicodeScalars.count
            if !pattern.isEmpty && size > length && padded.contains(" " + pattern) {
                found = rule
                length = size
            }
        }
        return found
    }

    /// Words card processors and banks put before the merchant's name.
    private static let prefixes: Set<String> = ["pos", "debit", "credit", "card", "purchase", "checkcard", "visa", "ach", "recurring",
                                                 "pending", "preauth", "authorized", "sq", "tst", "sp", "pp", "paypal"]

    private static func digits(_ word: String) -> Int { word.count { $0.isNumber } }

    /// The part of some bank text worth matching on: without the processor's prefix ("SQ *",
    /// "PURCHASE AUTHORIZED ON 09/14"), and up to the first store number or date, at most three
    /// words. Words in `payee` are kept, so "PAYPAL" stays in a rule for PayPal.
    static func suggestMatch(_ raw: String, payee: String = "") -> String {
        let keep = Set(normalize(payee).split(separator: " ").map(String.init))
        let words = normalize(raw).split(separator: " ").map(String.init)
        func prefix(_ i: Int) -> Bool {
            !keep.contains(words[i])
                && (prefixes.contains(words[i]) || digits(words[i]) >= 2 || (words[i] == "on" && i > 0 && words[i - 1] == "authorized"))
        }
        var start = 0
        while start < words.count - 1 && prefix(start) { start += 1 }
        var picked: [String] = []
        for word in words[start...] {
            if digits(word) >= 2 || picked.count == 3 { break }
            picked.append(word)
        }
        return (picked.isEmpty ? words : picked).joined(separator: " ")
    }

    /// A rule that makes `raw` come out as `payee` from now on, or nil if the rules already do:
    /// the suggested text, or the bank's whole text when an existing rule would win over that.
    static func ruleFor(_ rules: [JSON], raw: String, payee: String) -> JSON? {
        let name = payee.trimmingCharacters(in: .whitespaces)
        guard !raw.trimmingCharacters(in: .whitespaces).isEmpty, !name.isEmpty,
              rule(rules, for: raw)?.text("payee").trimmingCharacters(in: .whitespaces) != name else { return nil }
        let marker = "candidate:\(UUID().uuidString)"
        for match in [suggestMatch(raw, payee: name), normalize(raw)] where !match.isEmpty {
            var candidate: JSON = ["id": .string(marker), "match": .string(match), "payee": .string(name), "categoryId": .null]
            // It has to be the rule that wins for this text, over every rule already there.
            if rule(rules + [candidate], for: raw)?.text("id") == marker {
                candidate["id"] = ""
                return candidate
            }
        }
        return nil
    }

    /// After a transaction from a bank file is saved under a new payee, the rule that would
    /// remember it (nil for a transfer, a transaction typed by hand, or an unchanged name).
    static func learn(original: JSON?, payee: String, transfer: Bool, rules: [JSON]) -> JSON? {
        guard let original, !transfer else { return nil }
        let raw = original.text("importedPayee")
        guard !raw.trimmingCharacters(in: .whitespaces).isEmpty, original.text("payee") != payee else { return nil }
        return ruleFor(rules, raw: raw, payee: payee)
    }
}

extension BudgetStore {
    /// Offers to remember a renamed bank payee as a rule, then to rename earlier transactions with
    /// that bank text too, as the desktop's useLearnPayee does.
    func offerPayeeRule(_ learned: JSON) {
        var rule = learned
        rule["id"] = .string(UUID().uuidString.lowercased())
        let offer = rule
        let payee = rule.text("payee"), match = rule.text("match")
        showToast(String(localized: "Always use \(payee) for bank payees with “\(match)”?"),
                  action: ToastAction(label: String(localized: "Add rule")) { self.addPayeeRule(offer) })
    }

    private func addPayeeRule(_ rule: JSON) {
        Task {
            do {
                try await editing { budget in
                    var rules = budget["payeeRules"].array
                    rules.append(rule)
                    budget["payeeRules"] = .array(rules)
                }
                let id = rule.text("id")
                let budget = budget
                let others = await Task.detached(priority: .userInitiated) {
                    budget.flatMap { try? PayeeRulePreview.changes(budget: $0, only: [id]) } ?? 0
                }.value
                if others > 0 {
                    showToast(Self.applyRuleOffer(others),
                              action: ToastAction(label: String(localized: "Apply")) { self.reapplyPayeeRule(id, count: others) })
                } else {
                    showToast(String(localized: "Rule added."), undo: true)
                }
            } catch {
                self.error = Self.readable(error)
            }
        }
    }

    private func reapplyPayeeRule(_ id: String, count: Int) {
        workflow(["type": "reapplyPayeeRules", "ruleIds": JSON([id])]) {
            self.showToast(Self.updatedText(count), undo: true)
        }
    }

    /// "Rule added. Apply it to 3 earlier transactions too?", grouped from 1,000 as counts always were.
    private static func applyRuleOffer(_ count: Int) -> String {
        if count < 1000 { return String(localized: "Rule added. Apply it to \(count) earlier transactions too?", locale: .app) }
        let grouped = count.formatted(.number.locale(.app))
        return String(localized: "Rule added. Apply it to \(grouped) earlier transactions too?")
    }

    /// "Updated 3 transactions.", grouped from 1,000 as counts always were.
    private static func updatedText(_ count: Int) -> String {
        if count < 1000 { return String(localized: "Updated \(count) transactions.", locale: .app) }
        let grouped = count.formatted(.number.locale(.app))
        return String(localized: "Updated \(grouped) transactions.")
    }
}
