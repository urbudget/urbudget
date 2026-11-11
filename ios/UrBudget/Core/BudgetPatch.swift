import Foundation

/// The Swift twin of crates/urbudget-core/src/patch.rs: the patches `edit_budget` replies with, and
/// the ones an edit made here sends. Android's is BudgetPatch.kt. `JSON` is a value, so a patched
/// budget shares every record the patch didn't touch with the one before it.
enum BudgetPatch {
    private static let scalars = ["schemaVersion", "name", "currency"]
    private static let collections = ["accounts", "categories", "transactions"]
    private static let typed: Set<String> = ["schemaVersion", "name", "currency", "accounts", "categories", "transactions", "assignments"]

    /// `base` with `patch` applied. An empty patch gives `base` back.
    static func apply(_ patch: JSON, to base: JSON) -> JSON {
        guard case .object(let change) = patch, !change.isEmpty, case .object(var next) = base else { return base }
        for key in scalars {
            if let value = change[key] { next.updateValue(value, forKey: key) }
        }
        for key in collections {
            if let rows = change[key], rows.isObject { next.updateValue(.array(collection(next[key]?.array ?? [], rows)), forKey: key) }
        }
        if let rows = change["assignments"], rows.isObject {
            next.updateValue(.array(assignments(next["assignments"]?.array ?? [], rows)), forKey: "assignments")
        }
        if let extras = change["extras"], extras.isObject {
            for key in extras["delete"].array { next.removeValue(forKey: key.string) }
            for (key, value) in extras["set"].object { next.updateValue(value, forKey: key) }
        }
        return .object(next)
    }

    private static func collection(_ base: [JSON], _ change: JSON) -> [JSON] {
        let gone = Set(change["delete"].array.map(\.string))
        var rows = base.filter { $0.isObject && !gone.contains($0.text("id")) }
        var index: [String: Int] = [:]
        for (at, row) in rows.enumerated() { index[row.text("id")] = at }
        for row in change.rows("upsert") {
            let id = row.text("id")
            if let at = index[id] { rows[at] = row } else { index[id] = rows.count; rows.append(row) }
        }
        if case .array(let order) = change["order"] {
            var rank: [String: Int] = [:]
            for (at, id) in order.enumerated() { rank[id.string] = at }
            // Stable: ids the order doesn't name go last, as they lay.
            rows = rows.enumerated().sorted { a, b in
                let left = rank[a.element.text("id")] ?? Int.max, right = rank[b.element.text("id")] ?? Int.max
                return left == right ? a.offset < b.offset : left < right
            }.map(\.element)
        }
        return rows
    }

    private struct AssignmentKey: Hashable {
        var month: String
        var category: String
        init(_ row: JSON) { month = row.text("month"); category = row.text("categoryId") }
    }

    private static func assignments(_ base: [JSON], _ change: JSON) -> [JSON] {
        let gone = Set(change.rows("delete").map(AssignmentKey.init))
        var rows = base.filter { $0.isObject && !gone.contains(AssignmentKey($0)) }
        var index: [AssignmentKey: Int] = [:]
        for (at, row) in rows.enumerated() { index[AssignmentKey(row)] = at }
        for row in change.rows("upsert") {
            let key = AssignmentKey(row)
            if let at = index[key] { rows[at] = row } else { index[key] = rows.count; rows.append(row) }
        }
        return rows
    }

    /// What turned `base` into `edited`: a patch of whole records, or an empty object. Records are
    /// compared by value; the engine diffs what it ends up with, so a record sent unchanged costs
    /// nothing but its size.
    static func diff(from base: JSON, to edited: JSON) -> JSON {
        let before = base.object, after = edited.object
        var patch: [String: JSON] = [:]
        for key in scalars {
            if let value = after[key], before[key] != value { patch[key] = value }
        }
        for key in collections where before[key] != after[key] {
            if let change = collectionChange(base.rows(key), edited.rows(key)) { patch[key] = change }
        }
        if before["assignments"] != after["assignments"], let change = assignmentChange(base.rows("assignments"), edited.rows("assignments")) {
            patch["assignments"] = change
        }
        var set: [String: JSON] = [:]
        for (key, value) in after where !typed.contains(key) && before[key] != value { set[key] = value }
        let delete = before.keys.filter { !typed.contains($0) && after[$0] == nil }.sorted()
        if !set.isEmpty || !delete.isEmpty { patch["extras"] = ["set": .object(set), "delete": JSON(delete)] }
        return .object(patch)
    }

    private static func collectionChange(_ before: [JSON], _ after: [JSON]) -> JSON? {
        var previous: [String: JSON] = [:]
        for row in before { previous[row.text("id")] = row }
        let order = after.map { $0.text("id") }
        let present = Set(order)
        let upsert = after.filter { previous[$0.text("id")] != $0 }
        let delete = before.map { $0.text("id") }.filter { !present.contains($0) }
        // Without an order, survivors keep their place and new records go last.
        let implied = before.map { $0.text("id") }.filter(present.contains) + order.filter { previous[$0] == nil }
        if upsert.isEmpty && delete.isEmpty && implied == order { return nil }
        var change: JSON = ["upsert": .array(upsert), "delete": JSON(delete)]
        if implied != order { change["order"] = JSON(order) }
        return change
    }

    private static func assignmentChange(_ before: [JSON], _ after: [JSON]) -> JSON? {
        var previous: [AssignmentKey: JSON] = [:]
        for row in before { previous[AssignmentKey(row)] = row }
        let present = Set(after.map(AssignmentKey.init))
        let upsert = after.filter { previous[AssignmentKey($0)] != $0 }
        let delete: [JSON] = before.filter { !present.contains(AssignmentKey($0)) }
            .map { ["month": .string($0.text("month")), "categoryId": .string($0.text("categoryId"))] }
        if upsert.isEmpty && delete.isEmpty { return nil }
        return ["upsert": .array(upsert), "delete": .array(delete)]
    }

    /// Whether two copies hold the same budget. A reload sorts assignments; a patched copy keeps
    /// them where they were added.
    static func same(_ left: JSON, _ right: JSON) -> Bool {
        func sorted(_ budget: JSON) -> JSON {
            var copy = budget
            copy["assignments"] = .array(budget.rows("assignments").sorted {
                ($0.text("month"), $0.text("categoryId")) < ($1.text("month"), $1.text("categoryId"))
            })
            return copy
        }
        return sorted(left) == sorted(right)
    }
}
