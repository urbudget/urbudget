import AppIntents
import SwiftUI
import UIKit

/// Something asked of the app from outside it: the Home Screen quick action, or Siri, Spotlight,
/// the Action button and Shortcuts through the app's shortcut. The root view carries it out.
@MainActor @Observable
final class AppActions {
    enum Action: Equatable { case newTransaction }

    static let shared = AppActions()
    static let newTransactionType = "app.urbudget.newTransaction"

    /// The action waiting for the app's screens, cleared once one takes it.
    var pending: Action?

    @discardableResult
    func handle(_ item: UIApplicationShortcutItem) -> Bool {
        guard item.type == Self.newTransactionType else { return false }
        pending = .newTransaction
        return true
    }
}

/// "Add a transaction": opens UrBudget on a new transaction.
struct AddTransactionIntent: AppIntent {
    static var title: LocalizedStringResource = "Add a transaction"
    static var description = IntentDescription("Opens UrBudget on a new transaction.")
    static var openAppWhenRun = true

    @MainActor
    func perform() async throws -> some IntentResult {
        AppActions.shared.pending = .newTransaction
        return .result()
    }
}

struct UrBudgetShortcuts: AppShortcutsProvider {
    static var appShortcuts: [AppShortcut] {
        AppShortcut(intent: AddTransactionIntent(),
                    phrases: ["Add a transaction in \(.applicationName)", "New \(.applicationName) transaction"],
                    shortTitle: "Add transaction", systemImageName: "plus.circle")
    }
}

/// Hands the Home Screen quick action to `AppActions`, at launch and while the app runs.
final class AppDelegate: NSObject, UIApplicationDelegate {
    func application(_ application: UIApplication, didFinishLaunchingWithOptions options: [UIApplication.LaunchOptionsKey: Any]? = nil) -> Bool {
        MainActor.assumeIsolated { SyncBackground.register() }
        return true
    }

    func application(_ application: UIApplication, configurationForConnecting session: UISceneSession,
                     options: UIScene.ConnectionOptions) -> UISceneConfiguration {
        if let item = options.shortcutItem { MainActor.assumeIsolated { _ = AppActions.shared.handle(item) } }
        let configuration = UISceneConfiguration(name: nil, sessionRole: session.role)
        configuration.delegateClass = QuickActionSceneDelegate.self
        return configuration
    }
}

final class QuickActionSceneDelegate: NSObject, UIWindowSceneDelegate {
    func windowScene(_ windowScene: UIWindowScene, performActionFor shortcutItem: UIApplicationShortcutItem,
                     completionHandler: @escaping (Bool) -> Void) {
        completionHandler(MainActor.assumeIsolated { AppActions.shared.handle(shortcutItem) })
    }
}
