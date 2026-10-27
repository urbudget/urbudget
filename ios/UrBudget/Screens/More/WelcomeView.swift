import SwiftUI
import UniformTypeIdentifiers

/// The first screen: create a budget, try the sample, or bring a backup from desktop or Android.
struct WelcomeView: View {
    @Environment(BudgetStore.self) private var store
    @State private var name = String(localized: "My budget")
    @State private var currency = Locale.current.currency?.identifier ?? "SAR"
    @State private var importing = false
    @State private var fileError: String?
    @State private var starter = true
    @State private var decimal = StarterBudget.localFormats(currency: "SAR").number.decimal
    @State private var dateFormat = StarterBudget.localFormats(currency: "SAR").date

    var body: some View {
        NavigationStack {
            Form {
                Section {
                    VStack(alignment: .leading, spacing: 14) {
                        Text(verbatim: "Ur")
                            .font(.system(size: 26, weight: .heavy, design: .rounded))
                            .foregroundStyle(.white)
                            .frame(width: 64, height: 64)
                            .background(Color.urBrand, in: RoundedRectangle(cornerRadius: 16, style: .continuous))
                        Text("Make room for\nwhat matters.").font(.largeTitle.bold()).fixedSize(horizontal: false, vertical: true)
                        Text("A plan for every part of your money. Your budget lives on this device, and your desktop backups work here too.")
                            .foregroundStyle(Color.secondary)
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
                    Toggle(isOn: $starter) {
                        VStack(alignment: .leading, spacing: 2) {
                            Text("Start with categories")
                            Text("For bills, needs, wants and savings. Rename or delete any of them later.").font(.footnote).foregroundStyle(Color.secondary)
                        }
                    }
                    Button(action: create) {
                        HStack {
                            Text("Create my budget").fontWeight(.semibold)
                            if store.busy { Spacer(); ProgressView() }
                        }
                    }
                    .disabled(store.busy || name.trimmingCharacters(in: .whitespaces).isEmpty)
                } footer: {
                    Text("Next, add the accounts your money is in. You can change the formats in Settings.")
                }
                Section {
                    Button { store.createBudget(name: String(localized: "Sample budget"), currency: currency, sample: true) } label: {
                        Label { rowText("Explore with sample money", "A filled-in budget to try things on") } icon: { Image(systemName: "sparkles") }
                    }
                    Button { importing = true } label: {
                        Label { rowText("Import UrBudget backup", "A JSON backup from desktop or Android") } icon: { Image(systemName: "square.and.arrow.down") }
                    }
                    NavigationLink { AccountSyncView() } label: {
                        Label { rowText("Sign in and sync", "Bring down a budget you sync from desktop or Android") } icon: { Image(systemName: "icloud.and.arrow.down") }
                    }
                }
                .disabled(store.busy)
                FormErrorView(message: fileError)
            }
            .fileImporter(isPresented: $importing, allowedContentTypes: [.json, .plainText, .data]) { result in
                readBackup(result) { store.importBudget($0) }
            }
        }
    }

    /// A new budget in this device's formats, with starter categories if wanted, and the getting-started steps.
    private func create() {
        let local = StarterBudget.localFormats(currency: currency)
        let number = local.number.with(decimal: decimal)
        let date = dateFormat
        let starter = starter
        store.setUpFromWelcome = true
        store.createBudget(name: name, currency: currency) { budget in
            number.write(into: &budget)
            budget["dateFormat"] = .string(date)
            if starter { StarterBudget.addCategories(to: &budget) }
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
