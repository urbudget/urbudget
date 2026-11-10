import Foundation

extension Notification.Name {
    /// A UI effect the Rust services asked for: open a web page, assistant progress, sync state.
    /// `userInfo["event"]` is its name (a `String`), `userInfo["payload"]` its payload (a `JSON`).
    static let urbudgetHostEvent = Notification.Name("UrBudgetHostEvent")
}

/// Drains the services' `host_events` while the app is in the foreground, as Android's
/// NativeServices.pollEvents does, and posts each one as a `.urbudgetHostEvent` notification.
/// It carries on in the background only for the time iOS grants the assistant to finish a reply.
@MainActor
final class HostEvents {
    static let shared = HostEvents()
    private var task: Task<Void, Never>?
    /// Set by the assistant while a reply is being written: its text arrives in small pieces,
    /// so events are read ten times a second and it shows as it's written. Otherwise they are
    /// few and can wait a little longer.
    var fast = false

    func start() {
        guard task == nil else { return }
        task = Task {
            while !Task.isCancelled {
                // Before the services start this fails harmlessly; events wait in Rust until drained.
                if let events = try? await Task.detached(operation: { try NativeCore.service("host_events") }).value {
                    for event in events.array {
                        NotificationCenter.default.post(name: .urbudgetHostEvent, object: nil,
                                                        userInfo: ["event": event.text("event"), "payload": event["payload"]])
                    }
                }
                try? await Task.sleep(for: .milliseconds(fast ? 100 : 700))
            }
        }
    }

    func stop() {
        task?.cancel()
        task = nil
    }
}

extension Notification {
    /// The host event's name and payload, for `.onReceive` handlers.
    var hostEvent: (name: String, payload: JSON)? {
        guard let name = userInfo?["event"] as? String else { return nil }
        return (name, userInfo?["payload"] as? JSON ?? .null)
    }
}
