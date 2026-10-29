import SwiftUI
import UniformTypeIdentifiers

/// Settings & data: appearance and device preferences, the open budget's name, currency and formats,
/// flag names, JSON backups that work with desktop and Android, the engine's CSV exports, and the version.
struct SettingsView: View {
    @Environment(BudgetStore.self) private var store
    @AppStorage("theme") private var theme = "system"
    /// Read by the store when the app comes to the foreground.
    @AppStorage("automaticSchedules") private var automaticSchedules = true
    /// Read by the transaction editor, as the desktop's preference of the same name.
    @AppStorage("newCleared") private var newCleared = false
    @State private var name = ""
    @State private var namingIdentity: String?
    @FocusState private var naming: Bool
    @State private var nameError: String?
    @State private var editingFlags = false
    @State private var export: LibraryExport?
    @State private var exporting = false
    @State private var importing = false
    @State private var pendingImport: Data?
    @State private var reportFrom = Dates.addMonths(Dates.currentMonth, -11)
    @State private var reportThrough = Dates.currentMonth
    /// Which export is being prepared, for its progress indicator.
    @State private var preparing: String?
    @State private var error: String?

    init() {}

    var body: some View {
        Form {
            Section("Appearance") {
                Picker("Theme", selection: $theme) {
                    Text("System").tag("system")
                    Text("Light").tag("light")
                    Text("Dark").tag("dark")
                }
                .pickerStyle(.segmented)
                languageRow
            }
            if let budget = store.budget {
                budgetSection(budget)
                formatSection(budget)
            }
            preferencesSection
            if store.budget != nil {
                Section("History") {
                    Button { store.undo() } label: { Label("Undo last change", systemImage: "arrow.uturn.backward") }.disabled(!store.undoAvailable)
                    Button { store.redo() } label: { Label("Redo", systemImage: "arrow.uturn.forward") }.disabled(!store.redoAvailable)
                }
            }
            backupsSection
            if store.budget != nil { exportSections }
            if let error { Section { FormErrorView(message: error) } }
            aboutSection
        }
        .navigationTitle("Settings & data")
        .inlineTitle()
        .onAppear { namingIdentity = store.identity; name = store.budget?.text("name") ?? "" }
        .retainingEditorDraft("budgetName", values: ["name": .string(name)], enabled: naming)
        .onChange(of: store.version) { if !naming { name = store.budget?.text("name") ?? "" } }
        .onChange(of: naming) { _, focused in if !focused { commitName() } }
        .onDisappear { commitName() }
        .sheet(isPresented: $editingFlags) { FlagNamesForm() }
        .fileExporter(isPresented: $exporting, document: export?.file, contentType: export?.type ?? .json, defaultFilename: export?.name) { result in
            switch result {
            case .success: store.showToast(export?.done ?? String(localized: "Exported"))
            case .failure(let failure): error = BudgetStore.readable(failure)
            }
        }
        .fileImporter(isPresented: $importing, allowedContentTypes: [.json, .plainText, .data]) { result in
            attempt($error) {
                let data = try readPickedFile(result)
                if store.budget == nil { store.importBudget(data) } else { pendingImport = data }
            }
        }
        .confirmationDialog("Replace the open budget?", isPresented: Binding(get: { pendingImport != nil }, set: { if !$0 { pendingImport = nil } }),
                            titleVisibility: .visible) {
            Button("Import backup", role: .destructive) {
                if let data = pendingImport { replace(with: data) }
                pendingImport = nil
            }
        } message: {
            Text("Import replaces the budget on this device. A copy of it is kept with its backups in the budget library, and you can undo the import.")
        }
    }

    // MARK: Sections

    /// The app's language, as iOS keeps it (Settings → UrBudget → Language): System by default.
    private var languageRow: some View {
        Button { openAppSettings() } label: {
            HStack {
                Text("Language").foregroundStyle(Color.primary)
                Spacer(minLength: 8)
                Text(AppLanguage.name).foregroundStyle(Color.secondary)
                Image(systemName: "arrow.up.forward.app").foregroundStyle(Color.secondary).accessibilityHidden(true)
            }
        }
        .accessibilityHint("Opens UrBudget in the Settings app, where you can choose its language")
    }

    private func budgetSection(_ budget: JSON) -> some View {
        Section {
            TextField("Budget name", text: $name)
                .wordsCapitalized()
                .focused($naming)
                .submitLabel(.done)
                .onSubmit(commitName)
            LibraryCurrencyPicker(selection: Binding(get: { budget.text("currency", "SAR") }, set: { code in
                store.edit { LibrarySettings.change(currency: code, in: &$0) }
            }))
            if let nameError { FormErrorView(message: nameError) }
        } header: {
            Text("Budget")
        } footer: {
            Text("Changing the currency changes labels. It does not convert amounts.")
        }
    }

    private func formatSection(_ budget: JSON) -> some View {
        let currency = budget.text("currency", "SAR")
        let format = LibraryNumberFormat(budget: budget)
        let dateFormat = budget.text("dateFormat", "iso")
        return Section {
            LabeledContent("Example") {
                Text("\(format.format(123_456, currency: currency)) · \(format.format(-5_000, currency: currency))")
                    .monospacedDigit()
            }
            .accessibilityElement(children: .combine)
            Picker("Decimal separator", selection: formatBinding(format.decimal) { $0 = $0.with(decimal: $1) }) {
                ForEach(LibraryNumberFormat.decimalChoices, id: \.id) { Text($0.label).tag($0.id) }
            }
            Picker("Thousands separator", selection: formatBinding(format.group) { $0.group = $1 }) {
                ForEach(format.groups, id: \.id) { Text($0.label).tag($0.id) }
            }
            Picker("Currency placement", selection: formatBinding(format.symbol) { $0.symbol = $1 }) {
                ForEach(LibraryNumberFormat.placementChoices, id: \.id) { Text($0.label).tag($0.id) }
            }
            Picker("Date format", selection: Binding(get: { dateFormat }, set: { value in
                store.edit { $0["dateFormat"] = .string(value) }
            })) {
                ForEach(LibrarySettings.dateFormats, id: \.self) { Text(LibrarySettings.dateExample($0, day: Dates.today)).tag($0) }
            }
        } header: {
            Text("Numbers and dates")
        } footer: {
            Text("How this budget writes amounts and dates, on this device and wherever else it opens.")
        }
    }

    private var preferencesSection: some View {
        Section {
            Toggle(isOn: $automaticSchedules) {
                VStack(alignment: .leading, spacing: 2) {
                    Text("Enter due schedules on opening")
                    Text("Scheduled transactions that are due are added when the app opens").font(.footnote).foregroundStyle(Color.secondary)
                }
            }
            Toggle(isOn: $newCleared) {
                VStack(alignment: .leading, spacing: 2) {
                    Text("New transactions start cleared")
                    Text("Useful if you enter transactions once they've reached your bank").font(.footnote).foregroundStyle(Color.secondary)
                }
            }
            if let budget = store.budget {
                Button { editingFlags = true } label: {
                    HStack(spacing: 12) {
                        VStack(alignment: .leading, spacing: 2) {
                            Text("Flag names").foregroundStyle(Color.primary)
                            Text(flagSummary(budget)).font(.footnote).foregroundStyle(Color.secondary).lineLimit(2)
                        }
                        Spacer(minLength: 8)
                        HStack(spacing: 3) { ForEach(flagColors, id: \.self) { FlagDot(color: $0, size: 8) } }
                            .accessibilityHidden(true)
                    }
                }
            }
        } header: {
            Text("Preferences")
        }
    }

    private var backupsSection: some View {
        Section {
            if store.budget != nil {
                Button { exportBackup() } label: {
                    rowLabel(String(localized: "Export full backup"), String(localized: "A portable JSON file with every desktop field"), "square.and.arrow.up")
                }
            }
            Button { importing = true } label: {
                rowLabel(String(localized: "Import full backup"), String(localized: "A UrBudget JSON budget from desktop or Android"), "square.and.arrow.down")
            }
            .disabled(store.busy)
        } header: {
            Text("Backups")
        } footer: {
            Text("Backups are portable: the same JSON opens on desktop and Android, and fields this app doesn't know are kept. Automatic backups and restores are in the budget library.")
        }
    }

    @ViewBuilder
    private var exportSections: some View {
        Section {
            exportButton(String(localized: "Register"), String(localized: "Every transaction, in YNAB's register columns"), "tablecells", key: "register") {
                try await store.query("exportRegister")
            } file: { ("urbudget-register.csv", String(localized: "Register exported")) }
            exportButton(String(localized: "Plan"), String(localized: "Assigned, activity and available for each category and month"), "square.grid.3x3", key: "plan") {
                try await store.query("exportPlan")
            } file: { ("urbudget-plan.csv", String(localized: "Plan exported")) }
        } header: {
            Text("CSV exports")
        } footer: {
            Text("Spreadsheets and YNAB can read both files.")
        }
        Section {
            MonthRow(title: String(localized: "From"), month: $reportFrom)
            MonthRow(title: String(localized: "Through"), month: $reportThrough)
            let from = reportFrom, through = reportThrough
            exportButton(String(localized: "Export report"), String(localized: "Income, spending and net worth by month, then each category and income source"),
                         "chart.bar.doc.horizontal", key: "report", enabled: from <= through) {
                try await store.query("reportCsv", ["from": .string(from), "to": .string(through)])
            } file: { ("urbudget-report-\(from)-\(through).csv", String(localized: "Report exported")) }
            if from > through { FormErrorView(message: String(localized: "Choose a From month on or before the Through month.")) }
        } header: {
            Text("Report CSV")
        }
    }

    private var aboutSection: some View {
        Section {
            LabeledContent("Version", value: version)
        } footer: {
            if let budget = store.budget {
                Text("\(budget.text("name")) · \(budget.text("currency")) · Stored on this device. Envelope math, targets, reports and imports use the same engine as the desktop app.")
            }
        }
    }

    // MARK: Parts

    private func rowLabel(_ title: String, _ detail: String, _ symbol: String) -> some View {
        Label {
            VStack(alignment: .leading, spacing: 2) {
                Text(title).foregroundStyle(Color.primary)
                Text(detail).font(.footnote).foregroundStyle(Color.secondary)
            }
        } icon: {
            Image(systemName: symbol)
        }
    }

    private func exportButton(_ title: String, _ detail: String, _ symbol: String, key: String, enabled: Bool = true,
                              _ make: @escaping @MainActor () async throws -> JSON, file: @escaping () -> (String, String)) -> some View {
        Button {
            exportCSV(key, make, file)
        } label: {
            HStack {
                rowLabel(title, detail, symbol)
                if preparing == key {
                    Spacer(minLength: 8)
                    ProgressView().accessibilityLabel("Preparing \(title)")
                }
            }
        }
        .disabled(!enabled || preparing != nil)
    }

    private func flagSummary(_ budget: JSON) -> String {
        let named = flagColors.compactMap { color -> String? in
            let name = budget["flagNames"].text(color)
            return name.isEmpty ? nil : name
        }
        return named.isEmpty ? String(localized: "Name colours such as Reimburse or Tax") : planningList(named)
    }

    private var version: String {
        let info = Bundle.main.infoDictionary
        let short = info?["CFBundleShortVersionString"] as? String ?? "—"
        guard let build = info?["CFBundleVersion"] as? String, build != short else { return short }
        return "\(short) (\(build))"
    }

    /// A number format change, made to the budget as it is when the edit runs, so quick changes don't undo each other.
    private func formatBinding(_ value: String, _ change: @escaping @Sendable (inout LibraryNumberFormat, String) -> Void) -> Binding<String> {
        Binding(get: { value }, set: { next in
            store.edit { budget in
                var format = LibraryNumberFormat(budget: budget)
                change(&format, next)
                format.write(into: &budget)
            }
        })
    }

    // MARK: Actions

    private func commitName() {
        guard namingIdentity == store.identity, let saved = store.budget?.text("name") else { return }
        let trimmed = name.trimmingCharacters(in: .whitespacesAndNewlines)
        guard trimmed != saved else { nameError = nil; return }
        guard LibrarySettings.validName(trimmed) else {
            nameError = trimmed.isEmpty ? String(localized: "Enter a budget name.") : String(localized: "Use a shorter budget name.")
            return
        }
        nameError = nil
        store.edit { $0["name"] = .string(trimmed) }
    }

    private func exportBackup() {
        Task {
            guard let data = await store.exportData() else { return }
            export = LibraryExport(file: TextFile(data: data), type: .json, name: "UrBudget-\(Dates.today).json", done: String(localized: "Backup exported"))
            exporting = true
        }
    }

    private func exportCSV(_ key: String, _ make: @escaping @MainActor () async throws -> JSON, _ file: @escaping () -> (String, String)) {
        preparing = key
        error = nil
        Task {
            do {
                let csv = try await make()
                let (name, done) = file()
                export = LibraryExport(file: TextFile(data: Data(csv.string.utf8)), type: .commaSeparatedText, name: name, done: done)
                exporting = true
            } catch {
                self.error = BudgetStore.readable(error)
            }
            preparing = nil
        }
    }

    /// Keeps a portable copy of the open budget in its backups, then imports over it (one undo step).
    private func replace(with data: Data) {
        Task {
            do {
                _ = try await store.service("backup_database")
            } catch {
                let reason = BudgetStore.readable(error)
                self.error = String(localized: "Nothing was imported: the open budget couldn't be backed up first. \(reason)")
                return
            }
            store.importBudget(data)
        }
    }
}

/// Names for the flag colours, such as Reimburse or Tax, saved in the budget for every device.
private struct FlagNamesForm: View {
    @Environment(BudgetStore.self) private var store
    @State private var names: [String: String] = [:]
    @State private var started = false

    var body: some View {
        FormSheet(title: String(localized: "Flag names")) {
            let edited = names
            try await store.editing { budget in LibrarySettings.set(flagNames: edited, in: &budget) }
        } content: {
            Section {
                ForEach(flagColors, id: \.self) { color in
                    HStack(spacing: 12) {
                        FlagDot(color: color, size: 12).accessibilityHidden(true)
                        TextField(Self.placeholder(color), text: Binding(
                            get: { names[color] ?? "" },
                            set: { names[color] = String($0.prefix(40)) }))
                    }
                }
            } footer: {
                Text("Use names such as Reimburse or Tax. Leave a name empty to use its color.")
            }
        }
        .presentationDetents([.medium, .large])
        .retainingEditorDraft("flagNames", values: .object(names.mapValues(JSON.string)))
        .onAppear {
            guard !started else { return }
            started = true
            let saved = store.budget?["flagNames"] ?? .null
            names = Dictionary(uniqueKeysWithValues: flagColors.map { ($0, saved.text($0)) })
        }
    }

    /// "Red flag": what an unnamed flag is called.
    private static func placeholder(_ color: String) -> String {
        switch color {
        case "red": return String(localized: "Red flag")
        case "orange": return String(localized: "Orange flag")
        case "yellow": return String(localized: "Yellow flag")
        case "green": return String(localized: "Green flag")
        case "blue": return String(localized: "Blue flag")
        case "purple": return String(localized: "Purple flag")
        default: return color.prefix(1).uppercased() + color.dropFirst() + " flag"
        }
    }
}
