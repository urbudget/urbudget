import Foundation

/// The versioned JSON boundary to the Rust engine and services (crates/urbudget-ios), the same
/// requests Android sends over JNI. All financial rules are implemented in urbudget-core.
enum NativeCore {
    private static let version: JSON = 1

    /// Pure engine requests (calculate, workflows, imports). Callable from any thread.
    static func engine(_ method: String, budget: JSON? = nil, params: JSON = [:]) throws -> JSON {
        var request: JSON = ["version": version, "method": .string(method), "params": params]
        if let budget { request["budget"] = budget }
        return try decode(call(urbudget_invoke, request.serialized()))
    }

    /// Opens the budget library and the sealed credential vault in `directory`.
    static func initialize(directory: URL) throws {
        _ = try decode(call(urbudget_initialize, directory.path))
    }

    /// Storage, account, sync, bank and assistant services. Blocks until done: call off the main thread.
    static func service(_ method: String, _ params: JSON = [:]) throws -> JSON {
        let request: JSON = ["version": version, "method": .string(method), "params": params]
        return try decode(call(urbudget_service, request.serialized()))
    }

    private static func call(_ function: (UnsafePointer<CChar>?) -> UnsafeMutablePointer<CChar>?, _ text: String) -> String {
        text.withCString { pointer in
            guard let reply = function(pointer) else {
                let nothing: JSON = ["ok": false, "error": .string(String(localized: "The engine returned nothing."))]
                return nothing.serialized()
            }
            defer { urbudget_free(reply) }
            return String(cString: reply)
        }
    }

    private static func decode(_ text: String) throws -> JSON {
        let reply = try JSON.parse(text)
        guard reply["ok"].bool else {
            throw AppError(reply.text("error", String(localized: "The service could not complete this request.")))
        }
        return reply["value"]
    }
}
