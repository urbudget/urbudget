import Foundation
import Observation
import StoreKit

/// Sync bought through the App Store with StoreKit 2. The app never decides whether Sync is on:
/// every transaction StoreKit hands over goes to the server, which checks Apple's signature, this
/// app, the product and the account the purchase was made for. The account overview then says
/// what's active, the same as for a subscription bought anywhere else.
@MainActor @Observable
final class AppStoreSubscription {
    static let productID = "app.urbudget.sync.monthly"

    /// The Sync product with the App Store's localized name and price, once loaded.
    private(set) var product: Product?
    /// The App Store couldn't list the product: offline, or not set up in App Store Connect.
    private(set) var unavailable = false
    /// Counts transactions the server took in the background, so an open screen can reload.
    private(set) var deliveries = 0

    @ObservationIgnored private weak var store: BudgetStore?
    @ObservationIgnored private var updates: Task<Void, Never>?

    func attach(_ store: BudgetStore) {
        self.store = store
    }

    /// Called whenever the app is in the foreground with its library open. The first time, it
    /// listens for transactions made outside a purchase here: renewals, Ask to Buy approvals,
    /// refunds and purchases on other devices; StoreKit also replays unfinished ones to it at
    /// launch. Later, it offers the server anything still unfinished, such as a purchase made
    /// while offline or signed out.
    func start() {
        if updates == nil {
            updates = Task { [weak self] in
                for await result in Transaction.updates { await self?.deliverQuietly(result) }
            }
        } else {
            Task { await deliverUnfinished() }
        }
    }

    func deliverUnfinished() async {
        for await result in Transaction.unfinished { await deliverQuietly(result) }
    }

    func loadProduct() async {
        guard product == nil else { return }
        do {
            product = try await Product.products(for: [Self.productID]).first
            unavailable = product == nil
        } catch {
            unavailable = true
        }
    }

    /// The UUID StoreKit records with a purchase, which ties it to the signed-in UrBudget account.
    func accountToken() async throws -> UUID {
        guard let store else { throw AppError(String(localized: "The App Store isn't available right now.")) }
        let context = try await store.service("billing_apple_context")
        guard let token = UUID(uuidString: context.text("appAccountToken")) else {
            throw AppError(String(localized: "UrBudget couldn't prepare the purchase. Try again in a minute."))
        }
        return token
    }

    /// What a purchase came to once the server has it: a message to show, or nil when cancelled.
    func complete(_ result: Product.PurchaseResult) async throws -> String? {
        switch result {
        case .success(let verification):
            let answer = try await deliver(verification)
            return answer["active"].bool
                ? String(localized: "Sync is on. Set up or unlock sync on this iPhone to start.")
                : String(localized: "The App Store took the purchase, but Sync isn't on yet. Try Restore purchases in a minute.")
        case .pending:
            return String(localized: "The purchase is waiting for approval. Sync turns on once it's approved.")
        case .userCancelled:
            return nil
        @unknown default:
            return nil
        }
    }

    /// Restore purchases: refreshes this Apple Account's transactions and sends the current one.
    /// Nil when the person closed the App Store's sign-in.
    func restore() async throws -> String? {
        do {
            try await AppStore.sync()
        } catch StoreKitError.userCancelled {
            return nil
        }
        var restored = false
        for await result in Transaction.currentEntitlements {
            guard case .verified(let transaction) = result, transaction.productID == Self.productID else { continue }
            restored = try await deliver(result)["active"].bool || restored
        }
        return restored ? String(localized: "Your App Store subscription to Sync is restored.") : String(localized: "This Apple Account has no active Sync subscription.")
    }

    /// Sends a verified Sync transaction to the server, and finishes it once the server has
    /// answered, accepted or refused. When the server can't be reached or nobody is signed in, it
    /// throws and leaves the transaction unfinished, so StoreKit offers it again.
    private func deliver(_ result: VerificationResult<Transaction>) async throws -> JSON {
        guard case .verified(let transaction) = result else { throw AppError(String(localized: "The App Store couldn't confirm this purchase.")) }
        guard let store else { throw AppError(String(localized: "The App Store isn't available right now.")) }
        let answer = try await store.service("billing_apple_verify", ["transaction": .string(result.jwsRepresentation)])
        await transaction.finish()
        if !answer.text("refused").isEmpty { throw AppError(answer.text("refused")) }
        return answer
    }

    private func deliverQuietly(_ result: VerificationResult<Transaction>) async {
        guard case .verified(let transaction) = result, transaction.productID == Self.productID,
              (try? await deliver(result)) != nil else { return }
        deliveries += 1
    }
}
