import StoreKit
import SwiftUI

/// Account & sync's Sync subscription: whether it's active, and buying, restoring or managing it
/// through the App Store. The account overview (`/me`) says what's active; StoreKit only sells.
struct SyncSubscriptionSection: View {
    /// The account overview, or nil while it loads.
    var overview: JSON?
    /// Loads the overview again after a purchase or restore.
    var reload: () async -> Void

    @Environment(AppStoreSubscription.self) private var appStore
    @Environment(\.purchase) private var purchase
    @State private var busy = false
    @State private var error: String?
    @State private var notice: String?
    @State private var managing = false

    private var plan: JSON { overview?["subscription"] ?? .null }
    private var provider: String { plan.text("provider") }
    private var active: Bool { overview?["sync"]["active"].bool ?? false }
    /// Sync given by UrBudget (the App Review account's) can still be bought, so the purchase can be tried.
    private var offered: Bool { !active || provider == "comp" }

    var body: some View {
        if overview?["sync"].isObject == true {
            Section {
                statusRow
                if let notice { Text(notice).font(.subheadline) }
                if let error { Text(error).font(.subheadline).foregroundStyle(Color.urNegative) }
                if provider == "apple" { Button("Manage subscription") { managing = true }.disabled(busy) }
                if offered { offerRows }
            } header: {
                Text("Sync subscription")
            } footer: {
                if offered { disclosures }
            }
            .task(id: offered) { if offered { await appStore.loadProduct() } }
            .manageSubscriptionsSheet(isPresented: $managing)
            .onChange(of: managing) { _, open in if !open { Task { await reload() } } }
        }
    }

    private var statusRow: some View {
        Label {
            VStack(alignment: .leading, spacing: 2) {
                Text(active ? "Sync subscription active" : "Sync needs an active subscription")
                if !statusDetail.isEmpty { Text(statusDetail).font(.footnote).foregroundStyle(Color.secondary) }
            }
        } icon: {
            Image(systemName: active ? "checkmark.icloud.fill" : "exclamationmark.icloud")
        }
        .foregroundStyle(active ? Color.urPositive : Color.urWarning)
    }

    private var statusDetail: String {
        switch provider {
        case "comp":
            return active ? String(localized: "Included with this account by UrBudget") : ""
        case "apple":
            if plan.text("status") == "unpaid" { return String(localized: "The App Store couldn't renew it. Check your payment method in Manage subscription.") }
            if let renews = date(plan.text("renewsAt")), active { return String(localized: "Renews \(renews)") }
            if let ends = date(plan.text("endsAt")) { return active ? String(localized: "Ends \(ends)") : String(localized: "Ended \(ends)") }
            return ""
        case "":
            return ""
        default:
            return active ? String(localized: "Manage it where you subscribed.") : ""
        }
    }

    @ViewBuilder private var offerRows: some View {
        if let product = appStore.product {
            VStack(alignment: .leading, spacing: 4) {
                Text(product.displayName).fontWeight(.semibold)
                if !product.description.isEmpty { Text(product.description).font(.subheadline).foregroundStyle(Color.secondary) }
            }
            Button { subscribe(product) } label: {
                HStack {
                    Text("Subscribe for \(product.displayPrice) a \(unit(product))")
                    if busy { Spacer(); ProgressView() }
                }
            }
            .fontWeight(.semibold)
            .disabled(busy)
        } else if appStore.unavailable {
            Text("The App Store isn't available right now. Try again later.").font(.subheadline).foregroundStyle(Color.secondary)
        } else {
            ProgressView()
        }
        Button("Restore purchases", action: restore).disabled(busy)
    }

    /// What App Review guideline 3.1.2 asks for beside the button: the subscription's name, length
    /// and price, that it renews automatically, and links to the terms and the privacy policy.
    @ViewBuilder private var disclosures: some View {
        VStack(alignment: .leading, spacing: 6) {
            if let product = appStore.product {
                Text("\(product.displayName): a \(length(product)) subscription for \(product.displayPrice) a \(unit(product)). Payment is charged to your Apple Account when you confirm the purchase. It renews automatically at the same price unless you turn off auto-renew at least 24 hours before the end of the current period. Manage or cancel it anytime in your Apple Account settings.")
            }
            HStack(spacing: 16) {
                Link("Terms of Use", destination: URL(string: "https://urbudget.app/terms/")!)
                Link("Privacy Policy", destination: URL(string: "https://urbudget.app/privacy/app/")!)
            }
        }
    }

    private func subscribe(_ product: Product) {
        run {
            let token = try await appStore.accountToken()
            notice = try await appStore.complete(try await purchase(product, options: [.appAccountToken(token)]))
        }
    }

    private func restore() {
        run { notice = try await appStore.restore() }
    }

    /// One App Store action at a time, with its error shown in the section; the overview reloads after.
    private func run(_ action: @escaping @MainActor () async throws -> Void) {
        guard !busy else { return }
        busy = true
        error = nil
        notice = nil
        Task {
            defer { busy = false }
            do { try await action() } catch { self.error = BudgetStore.readable(error) }
            await reload()
        }
    }

    /// What the price is for: "month" in "Subscribe for $2.99 a month", or "3 months".
    private func unit(_ product: Product) -> String {
        guard let period = product.subscription?.subscriptionPeriod else { return String(localized: "month") }
        guard period.value != 1 else { return name(period.unit) }
        let count = period.value
        switch period.unit {
        // Keys of their own: Arabic words these after "a" ("لكل") differently from a count on its own.
        case .day: return String(localized: "subscription.days", defaultValue: "\(count) days", locale: .app)
        case .week: return String(localized: "subscription.weeks", defaultValue: "\(count) weeks", locale: .app)
        case .month: return String(localized: "subscription.months", defaultValue: "\(count) months", locale: .app)
        case .year: return String(localized: "subscription.years", defaultValue: "\(count) years", locale: .app)
        @unknown default: return String(localized: "subscription.periods", defaultValue: "\(count) periods", locale: .app)
        }
    }

    /// How long the subscription runs: "1-month" in "a 1-month subscription".
    private func length(_ product: Product) -> String {
        guard let period = product.subscription?.subscriptionPeriod else { return String(localized: "1-month") }
        let count = period.value
        switch period.unit {
        case .day: return String(localized: "\(count)-day", locale: .app)
        case .week: return String(localized: "\(count)-week", locale: .app)
        case .month: return String(localized: "\(count)-month", locale: .app)
        case .year: return String(localized: "\(count)-year", locale: .app)
        @unknown default: return String(localized: "\(count)-period", locale: .app)
        }
    }

    private func name(_ unit: Product.SubscriptionPeriod.Unit) -> String {
        switch unit {
        case .day: return String(localized: "day")
        case .week: return String(localized: "week")
        case .month: return String(localized: "month")
        case .year: return String(localized: "year")
        @unknown default: return String(localized: "period")
        }
    }

    private func date(_ iso: String) -> String? {
        let parser = ISO8601DateFormatter()
        parser.formatOptions = [.withInternetDateTime, .withFractionalSeconds]
        let parsed = parser.date(from: iso) ?? ISO8601DateFormatter().date(from: iso)
        return parsed?.formatted(Date.FormatStyle(date: .abbreviated, time: .omitted).locale(.app))
    }
}
