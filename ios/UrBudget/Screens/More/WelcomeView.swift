import SwiftUI
import UniformTypeIdentifiers

/// The first screen: create a budget, try the sample, or bring a backup from desktop or Android.
/// The sample's banner opens it again, to leave the sample for a budget of your own.
struct WelcomeView: View {
    /// Set when leaving the sample budget (see `SampleBanner`).
    var leaving: SampleExit?
    /// With no budget open: the budgets this device has (kept copies, the trash, ones in the
    /// account), from the top bar's trailing end.
    var openLibrary: (() -> Void)?
    @Environment(BudgetStore.self) private var store
    @Environment(\.dismiss) private var dismiss
    @State private var name = String(localized: "My budget")
    @State private var currency = Locale.current.currency?.identifier ?? "SAR"
    @State private var importing = false
    @State private var fileError: String?
    @State private var template = BudgetTemplate.defaultID
    @State private var decimal = StarterBudget.localFormats(currency: "SAR").number.decimal
    @State private var dateFormat = StarterBudget.localFormats(currency: "SAR").date

    init(leaving: SampleExit? = nil, openLibrary: (() -> Void)? = nil) {
        self.leaving = leaving
        self.openLibrary = openLibrary
    }

    var body: some View {
        NavigationStack {
            CompactForm {
                Section {
                    VStack(alignment: .leading, spacing: 14) {
                        // The app's own mark, as on the Home Screen and on Android's welcome.
                        Image("Logo").resizable().interpolation(.high)
                            .frame(width: 64, height: 64)
                            .clipShape(RoundedRectangle(cornerRadius: 16, style: .continuous))
                            .accessibilityHidden(true)
                        Text("Make room for\nwhat matters.").font(.largeTitle.bold()).fixedSize(horizontal: false, vertical: true)
                        Text("A plan for every part of your money. Your budget lives on this device, and your desktop backups work here too.")
                            .foregroundStyle(Color.secondary)
                        // A quiet link: the guide on urbudget.app, for anyone who wants the method before starting.
                        Button("How this works") {
                            Task {
                                do { _ = try await store.service("account_open", ["next": .string(AppGuide.path)]) } catch { fileError = BudgetStore.readable(error) }
                            }
                        }
                        .buttonStyle(.borderless)
                    }
                    .padding(.vertical, 8)
                    .listRowBackground(Color.clear)
                    .listRowInsets(EdgeInsets(top: 8, leading: 4, bottom: 8, trailing: 4))
                }
                Section {
                    TextField("Budget name", text: $name).wordsCapitalized()
                    Picker("Currency", selection: $currency) {
                        ForEach(currencyChoices(currency), id: \.self) { Text($0).tag($0) }
                    }
                    Picker("Decimal separator", selection: $decimal) {
                        ForEach(LibraryNumberFormat.decimalChoices, id: \.id) { Text($0.label).tag($0.id) }
                    }
                    Picker("Date format", selection: $dateFormat) {
                        ForEach(LibrarySettings.dateFormats, id: \.self) { Text(LibrarySettings.dateExample($0, day: Dates.today)).tag($0) }
                    }
                    TemplatePicker(selection: $template)
                    // The one thing this screen is for, so it's the one filled button on it.
                    Button(action: create) {
                        HStack(spacing: 8) {
                            if store.busy { ProgressView().tint(.white) }
                            Text("Create my budget").fontWeight(.semibold)
                        }
                        .frame(maxWidth: .infinity, minHeight: 34)
                    }
                    .buttonStyle(.borderedProminent)
                    .buttonBorderShape(.capsule)
                    .listRowSeparator(.hidden)
                    .disabled(store.busy || name.trimmingCharacters(in: .whitespaces).isEmpty)
                } footer: {
                    Text("Next, add the accounts your money is in. You can change the formats in Settings."
                         + (leaving?.delete == true ? " The sample budget is deleted once yours opens." : ""))
                }
                Section {
                    if leaving == nil {
                        Button { store.createBudget(name: String(localized: "Sample budget"), currency: currency, sample: true) } label: {
                            Label { rowText("Explore with sample money", "A filled-in budget to try things on") } icon: { Image(systemName: "sparkles") }
                        }
                    }
                    Button { importing = true } label: {
                        Label { rowText("Import UrBudget backup", "A JSON backup from UrBudget on another device") } icon: { Image(systemName: "square.and.arrow.down") }
                    }
                    NavigationLink { AccountSyncView() } label: {
                        Label { rowText("Sign in and sync", "Bring down a budget you sync from another device") } icon: { Image(systemName: "icloud.and.arrow.down") }
                    }
                }
                .disabled(store.busy)
                FormErrorView(message: fileError)
            }
            .fileImporter(isPresented: $importing, allowedContentTypes: [.json, .plainText, .data]) { result in
                // Leaving the sample, a backup is a budget of its own rather than the sample's replacement.
                readBackup(result) { leaving == nil ? store.importBudget($0) : store.importAsNewBudget($0, then: leaveSample) }
            }
            .toolbar {
                if leaving != nil {
                    ToolbarItem(placement: .cancellationAction) { Button("Cancel") { dismiss() } }
                } else if let openLibrary {
                    ToolbarItem(placement: .topBarTrailing) {
                        // Named as well as drawn: a bar button's label alone would show only the folder.
                        Button(action: openLibrary) {
                            HStack(spacing: 6) {
                                Image(systemName: "folder")
                                Text("Budget library")
                            }
                        }
                    }
                }
            }
        }
    }

    /// A new budget in this device's formats, with the chosen template's categories, and the getting-started steps.
    private func create() {
        let local = StarterBudget.localFormats(currency: currency)
        let number = local.number.with(decimal: decimal)
        let date = dateFormat
        store.setUpFromWelcome = true
        store.createBudget(name: name, currency: currency, template: template, adjust: { budget in
            number.write(into: &budget)
            budget["dateFormat"] = .string(date)
        }, then: leaveSample)
    }

    /// Once a budget of your own has opened in place of the sample: closes this, and deletes the sample if asked.
    private func leaveSample() {
        guard let leaving else { return }
        dismiss()
        guard leaving.delete else { return }
        let store = store
        Task {
            do {
                _ = try await store.deleteBudget(id: leaving.id)
                store.showToast(String(localized: "Deleted the sample budget."))
            } catch {
                store.error = BudgetStore.readable(error)
            }
        }
    }

    private func rowText(_ title: LocalizedStringKey, _ detail: LocalizedStringKey) -> some View {
        VStack(alignment: .leading, spacing: 2) {
            Text(title).foregroundStyle(Color.primary)
            Text(detail).font(.footnote).foregroundStyle(Color.secondary)
        }
    }

    private func readBackup(_ result: Result<URL, Error>, _ use: (Data) -> Void) {
        attempt($fileError) { use(try readPickedFile(result)) }
    }
}

/// The getting-started guide as the phone apps open it: urbudget.app's copy without site navigation,
/// checkout or account links. Welcome and Settings both lead to it, as Android's do.
enum AppGuide {
    static let path = "/getting-started/app/"
}

/// The currencies offered when creating a budget, the device's first.
func currencyChoices(_ current: String) -> [String] {
    var seen: Set<String> = []
    return ([current, "SAR", "USD", "EUR", "GBP", "CAD", "AUD", "AED", "INR"]).filter { seen.insert($0).inserted }
}

/// Reads a file the person picked, within the 32 MB a backup may be.
func readPickedFile(_ result: Result<URL, Error>) throws -> Data {
    let url = try result.get()
    let scoped = url.startAccessingSecurityScopedResource()
    defer { if scoped { url.stopAccessingSecurityScopedResource() } }
    let size = (try? url.resourceValues(forKeys: [.fileSizeKey]).fileSize) ?? 0
    guard size <= 32 * 1024 * 1024 else { throw AppError(String(localized: "Choose a JSON backup smaller than 32 MB.")) }
    return try Data(contentsOf: url)
}
