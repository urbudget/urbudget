import SwiftUI

/// The assistant's provider, sign-in or API key, model, privacy options and history. Pushed in
/// a navigation stack; the assistant's own sheet pushes it too.
struct AssistantSettingsView: View {
    @Environment(BudgetStore.self) private var store
    private let controller = AssistantController.shared
    @State private var key = ""
    @State private var deletingHistory = false

    init() {}

    private static let providers = [("chatgpt", String(localized: "ChatGPT plan · browser sign-in")), ("openai", String(localized: "OpenAI API key")),
                                    ("anthropic", String(localized: "Anthropic API key"))]

    var body: some View {
        let status = controller.status
        let provider = controller.provider
        let ready = controller.ready
        let signingIn = controller.signingIn
        Form {
            Section {
                Text("Answers use your budget and the files you attach. Changes are proposed for your review. Your chosen provider processes the conversation.")
                    .font(.subheadline)
                if let error = controller.error { FormErrorView(message: error) }
            }
            Section("Provider") {
                Picker("Provider", selection: Binding(get: { provider }, set: { value in
                    guard !signingIn, value != provider else { return }
                    key = ""
                    controller.settings(["provider": .string(value)])
                })) {
                    ForEach(Self.providers, id: \.0) { Text($0.1).tag($0.0) }
                }
                .pickerStyle(.inline)
                .labelsHidden()
                .disabled(signingIn || status == nil)
            }
            if provider == "chatgpt" {
                chatGPT(status: status, ready: ready, signingIn: signingIn)
            } else {
                apiKey(provider: provider, status: status)
            }
            if ready {
                Section {
                    Picker("Model", selection: Binding(get: { status?.text("model") ?? "" }, set: { value in
                        controller.settings(["model": .string(value)])
                    })) {
                        ForEach(modelChoices(status), id: \.0) { Text($0.1).tag($0.0) }
                    }
                    Button {
                        controller.refreshModels()
                    } label: {
                        HStack {
                            Text("Refresh available models")
                            if controller.busy.contains("models") { Spacer(); ProgressView() }
                        }
                    }
                    .disabled(controller.busy.contains("models"))
                } header: {
                    Text("Model")
                }
            }
            Section {
                Toggle(isOn: Binding(get: { status?.flag("shareMemos", true) ?? true }, set: { controller.settings(["shareMemos": .bool($0)]) })) {
                    VStack(alignment: .leading, spacing: 2) {
                        Text("Share transaction memos")
                        Text("Include memo text in budget lookups sent to the provider.").font(.footnote).foregroundStyle(Color.secondary)
                    }
                }
                Toggle(isOn: Binding(get: { status?.flag("keepHistory", true) ?? true }, set: { controller.settings(["keepHistory": .bool($0)]) })) {
                    VStack(alignment: .leading, spacing: 2) {
                        Text("Keep conversation history")
                        Text("Saved only in this budget's folder. Existing history stays until deleted.").font(.footnote).foregroundStyle(Color.secondary)
                    }
                }
            } header: {
                Text("Privacy")
            }
            .disabled(status == nil)
            Section {
                Button("Delete all conversations and attachments", role: .destructive) { deletingHistory = true }
                    .disabled(controller.busy.contains("delete"))
            }
        }
        .navigationTitle("Assistant settings")
        .inlineTitle()
        .task {
            controller.connect(store)
            controller.refreshStatus()
        }
        .onDisappear {
            key = ""
            controller.dismissError()
        }
        .alert("Delete all assistant history?", isPresented: $deletingHistory) {
            Button("Delete all", role: .destructive) { controller.deleteAll() }
            Button("Cancel", role: .cancel) {}
        } message: {
            Text("This deletes every conversation and attached file for the open budget on this device. Running replies are stopped.")
        }
    }

    @ViewBuilder
    private func chatGPT(status: JSON?, ready: Bool, signingIn: Bool) -> some View {
        Section {
            if ready {
                Label {
                    Text("Signed in \(status?["account"].text("email") ?? "")")
                } icon: {
                    Image(systemName: "checkmark.circle.fill").foregroundStyle(Color.urPositive)
                }
            } else {
                Text("Sign in on the ChatGPT page that opens, then return here. Your ChatGPT plan must allow this app's usage.")
                    .font(.subheadline)
                    .foregroundStyle(Color.secondary)
            }
            if signingIn {
                HStack(spacing: 10) {
                    ProgressView()
                    Text("Waiting for ChatGPT…").foregroundStyle(Color.secondary)
                }
                Button("Cancel sign-in", role: .cancel) { controller.cancelSignIn() }
            } else if ready {
                Button("Sign out", role: .destructive) { controller.signOut() }
                    .disabled(controller.busy.contains("signOut"))
            } else {
                Button("Continue with ChatGPT") { controller.signIn() }
                    .fontWeight(.semibold)
                    .disabled(status == nil)
            }
        } header: {
            Text("ChatGPT")
        } footer: {
            storageNote(status: status, ready: ready)
        }
        Section {
            Button("Plan usage") { controller.openLink("usage") }
            Button("Connected apps") { controller.openLink("disconnect") }
        }
    }

    @ViewBuilder
    private func apiKey(provider: String, status: JSON?) -> some View {
        let company = provider == "openai" ? "OpenAI" : "Anthropic"
        let saved = status?.flag(provider == "openai" ? "hasOpenaiKey" : "hasAnthropicKey") == true
        let checking = controller.busy.contains("key")
        Section {
            Text("API usage is billed by \(company). The key is checked with your provider before saving.")
                .font(.subheadline)
                .foregroundStyle(Color.secondary)
            if saved {
                Label {
                    Text("Key saved")
                } icon: {
                    Image(systemName: "key.fill").foregroundStyle(Color.urPositive)
                }
                Button("Remove saved key", role: .destructive) { controller.forgetKey(provider: provider) }
                    .disabled(checking)
            } else {
                SecureField("API key", text: $key)
                    .textInputAutocapitalization(.never)
                    .autocorrectionDisabled()
                    .privacySensitive()
                    .disabled(checking)
                Button {
                    let entered = key.trimmingCharacters(in: .whitespacesAndNewlines)
                    controller.setKey(provider: provider, key: entered) { key = "" }
                } label: {
                    HStack {
                        Text(checking ? "Checking key…" : "Verify and save key").fontWeight(.semibold)
                        if checking { Spacer(); ProgressView() }
                    }
                }
                .disabled(key.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty || checking)
            }
        } header: {
            Text(company)
        } footer: {
            storageNote(status: status, ready: saved)
        }
        Section {
            Button("Get an API key") { controller.openLink(provider == "openai" ? "openaiKeys" : "anthropicKeys") }
        }
    }

    @ViewBuilder
    private func storageNote(status: JSON?, ready: Bool) -> some View {
        let storage = status?.text("storage") ?? ""
        if !storage.isEmpty {
            Text("Credentials: \(storage)")
        } else if ready {
            Text("Credentials are kept in memory until the app closes.")
        }
    }

    /// "Provider default" first, then the provider's models; a saved model the list lacks stays choosable.
    private func modelChoices(_ status: JSON?) -> [(String, String)] {
        var choices = [("", String(localized: "Provider default"))] + controller.models.map { ($0.text("id"), $0.text("name", $0.text("id"))) }
        let current = status?.text("model") ?? ""
        if !current.isEmpty && !choices.contains(where: { $0.0 == current }) { choices.append((current, current)) }
        var seen = Set<String>()
        return choices.filter { seen.insert($0.0).inserted }
    }
}
