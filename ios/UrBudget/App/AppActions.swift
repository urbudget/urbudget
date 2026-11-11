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

    /// Where a card of the assistant asks the app to go as its sheet closes: the Transactions
    /// tab showing some transactions alone, or else the Budget tab on a month. Each is its own,
    /// so asking for the same place again goes there again.
    struct Destination: Equatable {
        var id = UUID()
        var month = ""
        var transactions: [String] = []
    }

    /// The place waiting for the tabs to go to, cleared once they have.
    var destination: Destination?
    /// A request for the assistant from a message over the tabs (an import's "Categorize with
    /// the assistant"): the tabs open the assistant with it, and clear it when it closes.
    var asking: AssistantRequest?

    /// The assistant open beside the app on a wide window. Each opening is its own, so a new
    /// request starts in a panel made for it.
    struct Panel: Identifiable {
        let id = UUID()
        var request: AssistantRequest?
    }

    /// The assistant beside the app, or nil while it's closed (or open as a sheet).
    var panel: Panel?
    /// Whether the window has room for the assistant beside the app, as the tabs measure it.
    var besideFits = false

    /// Opens the assistant beside the app where it fits, and says whether it did; where it
    /// doesn't, the screen that asked presents it as a sheet.
    func openBeside(_ request: AssistantRequest?) -> Bool {
        guard besideFits else { return false }
        panel = Panel(request: request)
        return true
    }

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

    /// An iPhone stays upright; an unfolded Duo, and an iPad, turn with the hand (`FoldOrientation`).
    func application(_ application: UIApplication, supportedInterfaceOrientationsFor window: UIWindow?) -> UIInterfaceOrientationMask {
        MainActor.assumeIsolated { FoldOrientation.mask }
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
