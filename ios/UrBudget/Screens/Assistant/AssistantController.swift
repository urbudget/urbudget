import Foundation
import Observation
import PhotosUI
import SwiftUI

extension Notification.Name {
    /// Posted when ChatGPT sign-in finishes, fails or is cancelled, so an in-app browser that
    /// shows the sign-in page can close itself.
    static let assistantSignInFinished = Notification.Name("AssistantSignInFinished")
}

/// The assistant's presentation state, as Android's AssistantController. Providers, tools,
/// history, validation and proposals live in Rust (`assistant_*` services); this class only
/// follows them. One instance serves the chat and its settings, so a conversation survives
/// closing the sheet.
@MainActor @Observable
final class AssistantController {
    static let shared = AssistantController()

    private(set) var status: JSON?
    private(set) var models: [JSON] = []
    private(set) var threads: [JSON] = []
    private(set) var thread: JSON?
    private(set) var entries: [AssistantEntry] = []
    private(set) var running = false
    private(set) var attachments: [JSON] = []
    private(set) var busy: Set<String> = []
    /// The reply as it streams in; only the live reply reads it, so tokens don't redraw the history.
    private(set) var streaming = ""
    private(set) var thinking = ""
    var error: String?
    var notice: String?
    private(set) var undoProposal: String?
    /// Counts applied proposals and finished imports, for a success haptic.
    private(set) var successes = 0

    var threadId: String { thread?.text("id") ?? "" }
    var ready: Bool { status?.flag("ready") == true }
    var provider: String { status?.text("provider", "chatgpt") ?? "chatgpt" }
    var signingIn: Bool { busy.contains("signIn") || status?.flag("signingIn") == true }

    @ObservationIgnored private weak var store: BudgetStore?
    @ObservationIgnored private var identity: String?
    @ObservationIgnored private var connected = false
    @ObservationIgnored private var undoToken: String?
    @ObservationIgnored private let stream = AssistantStreamBuffer()
    @ObservationIgnored private var streamPublish: Task<Void, Never>?
    /// A finished message is in the buffer's past: keep showing it until the reloaded thread has it.
    @ObservationIgnored private var replyPending = false
    @ObservationIgnored private var reload: Task<Void, Never>?
    @ObservationIgnored private var presentation: AssistantThreadPresentation?
    @ObservationIgnored private var generation = 0
    @ObservationIgnored private var loadRequest = 0
    @ObservationIgnored private var signInCancelled = false

    init() {}

    /// Called by the screens: the budget whose conversations and services to use. Another
    /// budget's conversation never stays open after switching budgets.
    func connect(_ store: BudgetStore) {
        self.store = store
        if connected && identity != store.identity {
            newChat()
            threads = []
        }
        connected = true
        identity = store.identity
    }

    // MARK: Service calls

    private func call(_ method: String, _ params: JSON = [:], changesBudget: Bool = false) async throws -> JSON {
        guard let store else { throw AppError("Budget services are still starting. Try again.") }
        return try await store.service(method, params, changesBudget: changesBudget)
    }

    /// Runs `work` once at a time per label, showing its error as Android's operation does.
    private func operation(_ label: String, _ work: @escaping @MainActor () async throws -> Void) {
        guard !busy.contains(label) else { return }
        busy.insert(label)
        error = nil
        Task { @MainActor in
            defer { busy.remove(label) }
            do {
                try await work()
            } catch is CancellationError {
            } catch {
                self.error = Self.message(error)
            }
        }
    }

    private static func message(_ error: Error) -> String {
        let text = BudgetStore.readable(error)
        return text.isEmpty ? "The assistant could not complete that action." : text
    }

    private func proposalParams(_ thread: String, _ proposal: String) -> JSON {
        ["threadId": .string(thread), "proposalId": .string(proposal)]
    }

    private var month: String { store?.month ?? Dates.currentMonth }

    // MARK: Loading

    private func loadStatus() async throws {
        let previous = status
        let next = try await call("assistant_status")
        status = next
        if next.flag("ready") && (previous?.text("provider") != next.text("provider") || previous?.flag("ready") != true) {
            operation("models") { [self] in try await loadModels(refresh: false) }
        }
    }

    private func loadModels(refresh: Bool) async throws {
        do {
            models = try await call("assistant_models", ["refresh": .bool(refresh)]).array.filter(\.isObject)
        } catch {
            models = []
            if refresh { throw error }
        }
    }

    private func loadHistory() async throws {
        threads = try await call("assistant_threads").array.filter(\.isObject)
    }

    private func loadThread(_ id: String) async throws {
        let generation = generation
        guard !id.isEmpty, threadId == id else { return }
        loadRequest += 1
        let request = loadRequest
        let value = try await call("assistant_thread", ["id": .string(id)])
        let prior = presentation
        let prepared = await Task.detached(priority: .userInitiated) { AssistantThreadPresentation.make(value, previous: prior) }.value
        // A reply from a conversation left while this request ran must not navigate back, and an
        // older reload must not replace a newer one.
        guard threadId == id, generation == self.generation, request == loadRequest else { return }
        presentation = prepared
        let running = value.flag("running")
        if !running { clearStream() }
        thread = prepared.thread
        entries = prepared.entries
        self.running = running
        if running {
            replyPending = false
            publishStream()
        } else {
            streaming = ""
            thinking = ""
        }
    }

    /// Status, history and the open conversation; on appearing and returning to the app.
    func refresh() {
        operation("refresh") { [self] in
            try await loadStatus()
            try await loadHistory()
            let id = threadId
            if !id.isEmpty && threads.contains(where: { $0.text("id") == id }) {
                try await loadThread(id)
            } else if !id.isEmpty && !running {
                newChat()
            }
        }
    }

    func refreshStatus() { operation("status") { [self] in try await loadStatus() } }

    /// Tool and message bursts need one history fetch, not one per event.
    func reloadThread() {
        guard !threadId.isEmpty, reload == nil else { return }
        reload = Task { @MainActor in
            try? await Task.sleep(for: .milliseconds(80))
            reload = nil
            let id = threadId
            do { try await loadThread(id) } catch {
                if threadId == id && !(error is CancellationError) { self.error = Self.message(error) }
            }
        }
    }

    /// The saved conversations, for the history list.
    func reloadHistory() { operation("history") { [self] in try await loadHistory() } }

    // MARK: Streaming

    /// An `assistant://event` for the open conversation: tokens, steps, proposals and the end of a reply.
    func receive(_ name: String, _ event: JSON) {
        guard name == "assistant://event", !threadId.isEmpty, event.text("threadId") == threadId else { return }
        let type = event.text("type")
        switch type {
        case "started":
            running = true
        case "delta", "thinking":
            stream.append(event.text("text"), thinking: type == "thinking")
            scheduleStreamPublish()
        case "message", "step", "proposal", "error", "done":
            if type == "message" {
                stream.clearReply()
                replyPending = true
            }
            reloadThread()
            if type == "done" { reloadHistory() }
        default:
            break
        }
    }

    private func scheduleStreamPublish() {
        guard streamPublish == nil else { return }
        streamPublish = Task { @MainActor in
            try? await Task.sleep(for: .milliseconds(64))
            streamPublish = nil
            if !replyPending { publishStream() }
        }
    }

    private func publishStream() {
        guard let (reply, reasoning) = stream.drain() else { return }
        streaming = reply
        thinking = reasoning
    }

    private func clearStream() {
        streamPublish?.cancel()
        streamPublish = nil
        replyPending = false
        stream.clear()
    }

    // MARK: Conversations

    func dismissError() { error = nil }
    func dismissNotice() { notice = nil }

    func newChat() {
        generation += 1
        clearStream()
        presentation = nil
        thread = nil
        entries = []
        running = false
        attachments = []
        streaming = ""
        thinking = ""
        undoProposal = nil
        undoToken = nil
    }

    func openThread(_ id: String) {
        operation("open") { [self] in
            generation += 1
            clearStream()
            presentation = nil
            thread = ["id": .string(id)]
            entries = []
            running = false
            streaming = ""
            thinking = ""
            attachments = []
            undoProposal = nil
            undoToken = nil
            try await loadThread(id)
        }
    }

    func deleteThread(_ id: String) {
        operation("delete") { [self] in
            _ = try await call("assistant_delete_thread", ["id": .string(id)])
            if threadId == id { newChat() }
            try await loadHistory()
        }
    }

    func deleteAll() {
        operation("delete") { [self] in
            _ = try await call("assistant_delete_all")
            newChat()
            try await loadHistory()
        }
    }

    // MARK: Settings

    func settings(_ change: JSON) {
        operation("settings") { [self] in
            _ = try await call("assistant_settings", ["change": change])
            try await loadStatus()
        }
    }

    func refreshModels() { operation("models") { [self] in try await loadModels(refresh: true) } }

    /// Starts ChatGPT sign-in. The service asks the app to show ChatGPT's page (host://open-url)
    /// and waits on its one-time 127.0.0.1 listener until the page redirects back or it's cancelled.
    func signIn() {
        signInCancelled = false
        operation("signIn") { [self] in
            if var shown = status {
                shown["signingIn"] = true
                status = shown
            }
            defer { NotificationCenter.default.post(name: .assistantSignInFinished, object: nil) }
            do {
                _ = try await call("assistant_sign_in")
                try? await loadStatus()
            } catch {
                try? await loadStatus()
                // Cancelling here is the person's own choice, not a failure to report.
                if !signInCancelled { throw error }
            }
        }
    }

    func cancelSignIn() {
        signInCancelled = true
        operation("cancel") { [self] in
            _ = try await call("assistant_cancel_sign_in")
            try await loadStatus()
        }
    }

    func signOut() {
        operation("signOut") { [self] in
            do {
                _ = try await call("assistant_sign_out")
            } catch {
                try? await loadStatus()
                throw error
            }
            try await loadStatus()
        }
    }

    /// Checks the key with its provider and keeps it in the sealed vault. The key goes only to
    /// the service; `done` lets the form clear its field.
    func setKey(provider: String, key: String, done: @escaping @MainActor () -> Void) {
        operation("key") { [self] in
            _ = try await call("assistant_set_key", ["provider": .string(provider), "key": .string(key)])
            done()
            try await loadStatus()
        }
    }

    func forgetKey(provider: String) {
        operation("key") { [self] in
            _ = try await call("assistant_forget_key", ["provider": .string(provider)])
            try await loadStatus()
        }
    }

    /// One of the service's fixed pages ("usage", "disconnect", "openaiKeys", "anthropicKeys"),
    /// shown through the app's host://open-url handler.
    func openLink(_ link: String) {
        operation("link") { [self] in _ = try await call("assistant_open", ["link": .string(link)]) }
    }

    // MARK: Attachments

    /// Files chosen in Files, read and sent to the service one by one.
    func attach(files: [URL]) {
        operation("attach") { [self] in
            let generation = generation
            for url in files {
                guard attachments.count < AssistantAttachments.limit else { throw AppError("Attach up to five files per message.") }
                let file = try await Task.detached(priority: .userInitiated) { () throws -> (name: String, data: Data) in
                    let read = try AssistantAttachments.read(url)
                    return AssistantAttachments.prepare(name: read.name, data: read.data)
                }.value
                try await upload(file.name, file.data, generation: generation)
            }
        }
    }

    /// Photos from the library: formats the service reads stay as they are, others become JPEG.
    func attach(photos: [PhotosPickerItem]) {
        operation("attach") { [self] in
            let generation = generation
            for item in photos {
                guard attachments.count < AssistantAttachments.limit else { throw AppError("Attach up to five files per message.") }
                guard let data = try await item.loadTransferable(type: Data.self) else {
                    throw AppError("This photo couldn't be read. Choose another, or attach it as a file.")
                }
                guard data.count <= AssistantAttachments.maxBytes * 2 else { throw AppError("Choose a photo smaller than 50 MiB.") }
                let type = item.supportedContentTypes.first { $0.conforms(to: .image) }
                let number = attachments.count + 1
                let file = try await Task.detached(priority: .userInitiated) {
                    try AssistantAttachments.photo(data, type: type, number: number)
                }.value
                guard file.data.count <= AssistantAttachments.maxBytes else { throw AppError("Choose a file smaller than 25 MiB.") }
                try await upload(file.name, file.data, generation: generation)
            }
        }
    }

    private func upload(_ name: String, _ data: Data, generation: Int) async throws {
        let encoded = await Task.detached(priority: .userInitiated) { data.base64EncodedString() }.value
        let meta = try await call("assistant_attach", ["name": .string(name), "data": .string(encoded)])
        if generation == self.generation && attachments.count < AssistantAttachments.limit { attachments.append(meta) }
    }

    func removeAttachment(_ id: String) { attachments.removeAll { $0.text("id") == id } }

    // MARK: Messages

    func send(_ text: String, selected: [String], importing: JSON? = nil, done: @escaping @MainActor () -> Void) {
        operation("send") { [self] in
            guard selected.count <= 200 else { throw AppError("Select up to 200 transactions per message.") }
            let generation = generation
            clearStream()
            streaming = ""
            thinking = ""
            let params: JSON = ["threadId": threadId.isEmpty ? .null : .string(threadId), "text": .string(text),
                                "attachments": JSON(attachments.map { $0.text("id") }),
                                "screen": AssistantScreen.context(month: month, selected: selected, importing: importing)]
            let result = try await call("assistant_send", params).string
            guard generation == self.generation else { return }
            if threadId != result { thread = ["id": .string(result)] }
            running = true
            attachments = []
            done()
            try await loadThread(result)
            try await loadHistory()
        }
    }

    /// Asks again after an error or an interrupted reply, without a new message.
    func retry(importing: JSON? = nil) {
        operation("send") { [self] in
            let id = threadId
            _ = try await call("assistant_retry", ["threadId": .string(id), "screen": AssistantScreen.context(month: month, selected: [], importing: importing)])
            try await loadThread(id)
        }
    }

    func stop() {
        operation("stop") { [self] in
            let id = threadId
            _ = try await call("assistant_stop", ["threadId": .string(id)])
            try await loadThread(id)
        }
    }

    // MARK: Proposals

    /// Rechecks the proposal against the budget as it is now, then commits it as one change. A
    /// proposal the budget has moved past comes back refreshed, for another review.
    func apply(_ proposal: String) {
        operation("proposal") { [self] in
            let id = threadId
            let outcome = try await call("assistant_apply_commit", proposalParams(id, proposal), changesBudget: true)
            if !outcome["patch"].isNull {
                var token = outcome["snapshot"].text("token")
                if token.isEmpty { token = try await call("database_status").text("token") }
                undoToken = token
                undoProposal = outcome["undoAvailable"].bool ? proposal : nil
                notice = "Proposal applied"
                successes += 1
            } else {
                notice = "Your budget changed. Review the refreshed proposal before applying it."
            }
            try await loadThread(id)
        }
    }

    /// Undoes an applied proposal, only while it is still the budget's latest change.
    func undo(_ proposal: String) {
        operation("proposal") { [self] in
            let id = threadId
            let token = try await call("database_status").text("token")
            guard token == undoToken && proposal == undoProposal else {
                throw AppError("Your budget changed after this proposal. Use the budget's undo history to review subsequent changes.")
            }
            var params = proposalParams(id, proposal)
            params["expectedToken"] = .string(token)
            _ = try await call("assistant_undo_commit", params, changesBudget: true)
            undoToken = nil
            undoProposal = nil
            try await loadThread(id)
        }
    }

    /// The proposal made again with the person's choices on its card (`ProposalChoices`): rows
    /// left out, categories or amounts given.
    func revise(_ proposal: String, _ revision: JSON) {
        operation("proposal") { [self] in
            let id = threadId
            var params = proposalParams(id, proposal)
            params["revision"] = revision
            _ = try await call("assistant_revise", params)
            try await loadThread(id)
        }
    }

    func discard(_ proposal: String) {
        operation("proposal") { [self] in
            var params = proposalParams(threadId, proposal)
            params["status"] = "discarded"
            _ = try await call("assistant_mark", params)
            try await loadThread(threadId)
        }
    }

    /// The bank-file review imported a converted statement: its card is done.
    func imported(thread: String, proposal: String) {
        successes += 1
        operation("proposal") { [self] in
            var params = proposalParams(thread, proposal)
            params["status"] = "applied"
            _ = try await call("assistant_mark", params)
            if threadId == thread { try await loadThread(thread) }
        }
    }

    func report(_ proposal: String, done: @escaping @MainActor (JSON) -> Void) {
        operation("report") { [self] in done(try await call("assistant_report", proposalParams(threadId, proposal))) }
    }

    func importFile(_ proposal: String, done: @escaping @MainActor (JSON, String) -> Void) {
        operation("import") { [self] in
            let thread = threadId
            done(try await call("assistant_import", proposalParams(thread, proposal)), thread)
        }
    }

    func saveImport(_ proposal: String, done: @escaping @MainActor (String) -> Void) {
        operation("export") { [self] in done(try await call("assistant_save_import", proposalParams(threadId, proposal)).string) }
    }

    /// Writes the workbook (or a report's tables) in the budget's exports folder and returns its
    /// path. The app previews or shares the file itself, so `open` is never asked of the service.
    func spreadsheet(_ proposal: String, done: @escaping @MainActor (String) -> Void) {
        operation("export") { [self] in
            var params = proposalParams(threadId, proposal)
            params["open"] = false
            done(try await call("assistant_spreadsheet", params).string)
        }
    }

    /// Sends only the excerpt and explanation the person approved to UrBudget.
    func reportReply(text: String, reason: String, done: @escaping @MainActor () -> Void) {
        operation("reportReply") { [self] in
            let reference = try await call("assistant_report_reply", ["text": .string(text), "reason": .string(reason)]).string
            notice = "Report sent · \(reference)"
            done()
        }
    }
}
