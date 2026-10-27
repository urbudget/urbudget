import Foundation

/// The portable budget is JSON carrying fields this app doesn't know about. Records are edited in
/// place, so desktop and Android metadata survive a round trip. Integers stay exact: money is in
/// cents and a Double would round large balances.
enum JSON: Equatable, Sendable {
    case null
    case bool(Bool)
    case int(Int64)
    case double(Double)
    case string(String)
    case array([JSON])
    case object([String: JSON])
}

extension JSON {
    struct ParseError: LocalizedError {
        var errorDescription: String? { String(localized: "This file is not valid JSON.") }
    }

    /// Converts what JSONSerialization produces. NSNumber keeps booleans, integers and decimals apart.
    init(foundation value: Any) {
        switch value {
        case let number as NSNumber:
            if CFGetTypeID(number) == CFBooleanGetTypeID() {
                self = .bool(number.boolValue)
            } else if CFNumberIsFloatType(number) {
                self = .double(number.doubleValue)
            } else {
                self = .int(number.int64Value)
            }
        case let text as String: self = .string(text)
        case let list as [Any]: self = .array(list.map(JSON.init(foundation:)))
        case let object as [String: Any]: self = .object(object.mapValues(JSON.init(foundation:)))
        default: self = .null
        }
    }

    var foundation: Any {
        switch self {
        case .null: return NSNull()
        case .bool(let value): return value
        case .int(let value): return NSNumber(value: value)
        case .double(let value): return value
        case .string(let value): return value
        case .array(let list): return list.map(\.foundation)
        case .object(let object): return object.mapValues(\.foundation)
        }
    }

    static func parse(_ data: Data) throws -> JSON {
        do {
            return JSON(foundation: try JSONSerialization.jsonObject(with: data, options: [.fragmentsAllowed]))
        } catch {
            throw ParseError()
        }
    }

    static func parse(_ text: String) throws -> JSON { try parse(Data(text.utf8)) }

    func data() -> Data {
        (try? JSONSerialization.data(withJSONObject: foundation, options: [.fragmentsAllowed])) ?? Data("null".utf8)
    }

    func serialized() -> String { String(decoding: data(), as: UTF8.self) }
}

// MARK: Reading

extension JSON {
    subscript(key: String) -> JSON {
        get {
            if case .object(let object) = self { return object[key] ?? .null }
            return .null
        }
        set {
            // Dropping self's reference first lets the dictionary change in place.
            var object: [String: JSON]
            if case .object(let existing) = self { object = existing } else { object = [:] }
            self = .null
            object[key] = newValue
            self = .object(object)
        }
    }

    subscript(index: Int) -> JSON {
        if case .array(let list) = self, list.indices.contains(index) { return list[index] }
        return .null
    }

    var isNull: Bool { if case .null = self { return true } else { return false } }
    var isObject: Bool { if case .object = self { return true } else { return false } }

    func has(_ key: String) -> Bool {
        if case .object(let object) = self { return object[key] != nil }
        return false
    }

    /// Text as Android's optString reads it: numbers and booleans become their text, null is empty.
    var string: String {
        switch self {
        case .string(let value): return value
        case .int(let value): return String(value)
        case .double(let value): return String(value)
        case .bool(let value): return value ? "true" : "false"
        default: return ""
        }
    }

    /// A field's text, or `fallback` when it is missing or null.
    func text(_ key: String, _ fallback: String = "") -> String {
        let value = self[key]
        return value.isNull ? fallback : value.string
    }

    var int: Int64 {
        switch self {
        case .int(let value): return value
        case .double(let value): return value.isFinite && abs(value) < 9.2e18 ? Int64(value) : 0
        case .string(let value): return Int64(value) ?? 0
        default: return 0
        }
    }

    func long(_ key: String) -> Int64 { self[key].int }

    var bool: Bool {
        switch self {
        case .bool(let value): return value
        case .string(let value): return value == "true"
        default: return false
        }
    }

    /// A boolean field, or `fallback` when it is missing or null.
    func flag(_ key: String, _ fallback: Bool = false) -> Bool {
        let value = self[key]
        return value.isNull ? fallback : value.bool
    }

    var array: [JSON] { if case .array(let list) = self { return list } else { return [] } }
    var object: [String: JSON] { if case .object(let object) = self { return object } else { return [:] } }
    var keys: [String] { Array(object.keys) }

    /// The objects in an array field.
    func rows(_ key: String) -> [JSON] { self[key].array.filter(\.isObject) }

    func row(_ key: String, id: String) -> JSON? { rows(key).first { $0.text("id") == id } }

    /// Removes a field, leaving the rest of the object as it was.
    mutating func remove(_ key: String) {
        guard case .object(var object) = self else { return }
        self = .null
        object.removeValue(forKey: key)
        self = .object(object)
    }

    /// The field's value, or JSON null for an empty string.
    static func text(orNull value: String?) -> JSON {
        guard let value, !value.isEmpty else { return .null }
        return .string(value)
    }
}

// MARK: Literals

extension JSON: ExpressibleByStringLiteral, ExpressibleByIntegerLiteral, ExpressibleByBooleanLiteral,
    ExpressibleByArrayLiteral, ExpressibleByDictionaryLiteral {
    init(stringLiteral value: String) { self = .string(value) }
    init(integerLiteral value: Int64) { self = .int(value) }
    init(booleanLiteral value: Bool) { self = .bool(value) }
    init(arrayLiteral elements: JSON...) { self = .array(elements) }
    init(dictionaryLiteral elements: (String, JSON)...) {
        self = .object(Dictionary(elements, uniquingKeysWith: { _, last in last }))
    }
    init(_ value: String) { self = .string(value) }
    init(_ value: Int64) { self = .int(value) }
    init(_ value: Bool) { self = .bool(value) }
    init(_ values: [String]) { self = .array(values.map(JSON.string)) }
}

// MARK: Record editing

let editableCollections: Set<String> = ["accounts", "categories", "transactions", "schedules", "loans", "payeeRules", "payees"]

extension JSON {
    /// Merges edited fields into a record (adding it when new) and never drops fields this app doesn't know.
    mutating func upsertRecord(_ collection: String, _ patch: JSON) throws {
        guard editableCollections.contains(collection) else { throw AppError(String(localized: "Unsupported record collection.")) }
        let identity = collection == "loans" ? "accountId" : "id"
        let id = patch.text(identity)
        guard !id.isEmpty else { throw AppError(String(localized: "Every record needs an ID.")) }
        var rows = self[collection].array
        self[collection] = .null
        if let index = rows.firstIndex(where: { $0.text(identity) == id }) {
            var merged = rows[index]
            for (key, value) in patch.object { merged[key] = value }
            rows[index] = merged
        } else {
            rows.append(patch)
        }
        self[collection] = .array(rows)
    }

    mutating func removeRecord(_ collection: String, id: String) throws {
        guard editableCollections.contains(collection) else { throw AppError(String(localized: "Unsupported record collection.")) }
        let identity = collection == "loans" ? "accountId" : "id"
        let rows = self[collection].array.filter { $0.text(identity) != id }
        self[collection] = .array(rows)
    }
}

/// An error with a message written for the person using the app.
struct AppError: LocalizedError {
    let message: String
    init(_ message: String) { self.message = message }
    var errorDescription: String? { message }
}

/// Text that sheets can present by its value, such as an account id.
struct IdentifiedText: Identifiable, Hashable {
    var value: String
    var id: String { value }
}
