import Foundation
import Observation
import PhotosUI
import SwiftUI
import UIKit

extension Notification.Name {
    /// Posted when ChatGPT sign-in finishes, fails or is cancelled, so an in-app browser that
    /// shows the sign-in page can close itself.
    static let assistantSignInFinished = Notification.Name("AssistantSignInFinished")
}

/// The assistant's presentation state, as Android's AssistantController. Providers, tools,
/// history, validation and proposals live in Rust (`assistant_*` services); this class only
/// follows them. One instance serves the chat and its settings and follows the services' events
/// itself, so a conversation and the message being written survive closing the sheet.
@MainActor @Observable
final class AssistantController {
    static let shared = AssistantController()

    private(set) var status: JSON?
    /// The status couldn't be read, and there's none from before to go by.
    private(set) var statusFailed = false
    private(set) var models: [JSON] = []
    private(set) var threads: [JSON] = []
    private(set) var thread: JSON?
    private(set) var entries: [AssistantEntry] = []
    private(set) var running = false
    private(set) var attachments: [JSON] = []
    private(set) var busy: Set<String> = []
    /// The action running on a proposal card, by the proposal's id: only that card's button spins.
    private(set) var cardWork: [String: String] = [:]
    /// The reply as it streams in, read as Markdown like a finished one; only the live reply
    /// reads it, so tokens don't redraw the history.
    private(set) var liveBlocks: [AssistantMarkdownBlock] = []
    private(set) var thinking = ""
    /// What the assistant is doing now ("Looking at October"), shown while no text is arriving.
    private(set) var activity = ""
    /// The message being written. It's kept here, with the transactions selected for it, so
    /// swiping the sheet away doesn't lose it; each conversation has its own.
    var draft = ""
    private(set) var selected: [String] = []
    /// How many transactions another screen handed over when only the first 200 could be attached.
    private(set) var selectionTotal: Int?
    /// Changes when another conversation is opened or started, and not when a new one gets its
    /// id with its first message: the conversation's scroll view is rebuilt only for the former.
    private(set) var conversation = 0
    /// A reply finished or a card arrived while the assistant wasn't on screen.
    private(set) var unseen = false
    var error: String?
    var notice: String?
    private(set) var undoProposal: String?
    /// Counts applied proposals and finished imports, for a success haptic.
    private(set) var successes = 0

    var threadId: String { thread?.text("id") ?? "" }
    var ready: Bool { status?.flag("ready") == true }
    var provider: String { status?.text("provider", "chatgpt") ?? "chatgpt" }
    var signingIn: Bool { busy.contains("signIn") || status?.flag("signingIn") == true }
    /// The notice of what gets sent hasn't been read for the chosen service yet.
    var noticeDue: Bool { status?.flag("noticeDue") == true }

    @ObservationIgnored private weak var store: BudgetStore?
    @ObservationIgnored private var identity: String?
    @ObservationIgnored private var connected = false
    @ObservationIgnored private var undoToken: String?
    @ObservationIgnored private let stream = AssistantStreamBuffer()
    @ObservationIgnored private var streamPublish: Task<Void, Never>?
    /// A finished message is in the buffer's past: keep showing it until the reloaded thread has it.
    @ObservationIgnored private var replyPending = false
    /// The live reply's text as it should show, and the one reading of it into blocks under way.
    @ObservationIgnored private var liveText = ""
    @ObservationIgnored private var liveParse: Task<Void, Never>?
    @ObservationIgnored private var liveToken = 0
    @ObservationIgnored private var reload: Task<Void, Never>?
    @ObservationIgnored private var presentation: AssistantThreadPresentation?
    @ObservationIgnored private var generation = 0
    @ObservationIgnored private var loadRequest = 0
    @ObservationIgnored private var signInCancelled = false
    /// The messages being written in the conversations that aren't open, by conversation id
    /// ("" is a new conversation).
    @ObservationIgnored private var drafts: [String: AssistantDraft] = [:]
    /// The last request taken from another screen, and its files already uploaded for the
    /// message being written.
    @ObservationIgnored private var takenRequest: UUID?
    @ObservationIgnored private var requestFiles: Set<URL> = []
    /// How many of the assistant's sheets are on screen.
    @ObservationIgnored private var presented = 0
    /// The reply under way has written something to announce when it's done.
    @ObservationIgnored private var replied = false
    @ObservationIgnored private var inBackground = false
    @ObservationIgnored private var backgroundID: UIBackgroundTaskIdentifier = .invalid

    init() {
        // The conversation follows its events whether or not the sheet is up. They're posted on
        // the main thread (HostEvents), where this class lives.
        _ = NotificationCenter.default.addObserver(forName: .urbudgetHostEvent, object: nil, queue: .main) { [weak self] notification in
            guard let event = notification.hostEvent else { return }
            MainActor.assumeIsolated {
                guard let self else { return }
                self.receive(event.name, event.payload)
            }
        }
    }

    /// Called by the screens: the budget whose conversations and services to use. Another
    /// budget's conversation, and the message being written in it, never stay after switching budgets.
    func connect(_ store: BudgetStore) {
        self.store = store
        if connected && identity != store.identity {
            newChat()
            threads = []
            drafts = [:]
            clearDraft()
        }
        connected = true
        identity = store.identity
    }

    /// The assistant's sheet came on screen or left it. While it's on screen nothing is unseen.
    func shown(_ visible: Bool) {
        presented = max(0, presented + (visible ? 1 : -1))
        if presented > 0 && unseen { unseen = false }
    }

    // MARK: Service calls

    private func call(_ method: String, _ params: JSON = [:], changesBudget: Bool = false) async throws -> JSON {
        guard let store else { throw AppError(String(localized: "Budget services are still starting. Try again.")) }
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
        return text.isEmpty ? String(localized: "The assistant could not complete that action.") : text
    }

    private func proposalParams(_ thread: String, _ proposal: String) -> JSON {
        ["threadId": .string(thread), "proposalId": .string(proposal)]
    }

    /// Runs one action of a proposal card and marks that card while it runs, so only its button
    /// spins. Actions under the same `label` still run one at a time across the conversation.
    private func card(_ proposal: String, _ action: String, label: String = "proposal", _ work: @escaping @MainActor () async throws -> Void) {
        guard !busy.contains(label), cardWork[proposal] == nil else { return }
        cardWork[proposal] = action
        operation(label) { [self] in
            defer { cardWork[proposal] = nil }
            try await work()
        }
    }

    // MARK: Loading

    private func loadStatus() async throws {
        let previous = status
        let next: JSON
        do {
            next = try await call("assistant_status")
        } catch {
            if status == nil { statusFailed = true }
            throw error
        }
        if statusFailed { statusFailed = false }
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
        setRunning(running)
        if running {
            replyPending = false
            publishStream()
        } else {
            thinking = ""
        }
    }

    /// Status, history and the open conversation; on appearing and returning to the app.
    func refresh() {
        if statusFailed { statusFailed = false }
        operation("refresh") { [self] in
            do {
                try await loadStatus()
            } catch {
                // With no status to go by, the screen says it couldn't be loaded and offers
                // Try again itself: no alert on top of that.
                if status == nil { return }
                throw error
            }
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

    /// An `assistant://event` for the open conversation: tokens, what it's doing, steps,
    /// proposals and the end of a reply.
    private func receive(_ name: String, _ event: JSON) {
        guard name == "assistant://event", !threadId.isEmpty, event.text("threadId") == threadId else { return }
        let type = event.text("type")
        switch type {
        case "started":
            replied = false
            setRunning(true)
        case "delta", "thinking":
            // Only a reply under way has a live text. Events are polled, so tokens written just
            // before Stop can arrive after the conversation was read back with that text kept in
            // it, and would show a second time under it.
            guard running else { break }
            // Text is arriving: that says what it's doing better than the last lookup's name.
            if type == "delta" && !activity.isEmpty { activity = "" }
            stream.append(event.text("text"), thinking: type == "thinking")
            scheduleStreamPublish()
        case "activity":
            // As the live text, only for a reply under way: a late one would outlive it.
            if running { activity = event.text("text") }
        case "message", "step", "proposal", "error", "cut", "done":
            if !activity.isEmpty { activity = "" }
            if type == "message" {
                stream.clearReply()
                replyPending = true
                replied = true
            }
            reloadThread()
            if type == "done" { reloadHistory() }
            tell(type, stopped: event.flag("stopped"))
        default:
            // "replaced" and anything newer: the conversation shows it when it's next read.
            break
        }
    }

    /// Says a finished reply or a new card once: read aloud by VoiceOver, never the text as it
    /// streams, and as the dot on More's Assistant row when the assistant isn't on screen.
    private func tell(_ type: String, stopped: Bool) {
        switch type {
        case "proposal":
            AccessibilityNotification.Announcement(String(localized: "A proposal is ready to review.")).post()
            if presented == 0 { unseen = true }
        case "done":
            // A reply the person stopped, or one that only failed, isn't a reply to announce.
            let written = replied
            replied = false
            guard written, !stopped else { break }
            AccessibilityNotification.Announcement(String(localized: "The assistant replied.")).post()
            if presented == 0 { unseen = true }
        default:
            break
        }
    }

    /// A reply started or ended. While one is written events are read faster, so its text shows
    /// as it's written; when it ends, the time asked of iOS to finish it is given back.
    private func setRunning(_ value: Bool) {
        guard running != value else { return }
        running = value
        HostEvents.shared.fast = value
        if !value {
            if !activity.isEmpty { activity = "" }
            endBackgroundTime()
        }
    }

    private func scheduleStreamPublish() {
        guard streamPublish == nil else { return }
        streamPublish = Task { @MainActor in
            // About ten times a second: often enough to read as writing, and no more often than
            // the text can be read into blocks and drawn.
            try? await Task.sleep(for: .milliseconds(100))
            streamPublish = nil
            if !replyPending { publishStream() }
        }
    }

    private func publishStream() {
        guard let (reply, reasoning) = stream.drain() else { return }
        thinking = reasoning
        showLive(reply)
    }

    /// Shows the live reply as Markdown, read off the main actor one text at a time: while one
    /// is being read, later text waits and only the latest is read next.
    private func showLive(_ reply: String) {
        liveText = reply
        guard !reply.isEmpty else {
            liveToken += 1
            liveParse = nil
            if !liveBlocks.isEmpty { liveBlocks = [] }
            return
        }
        guard liveParse == nil else { return }
        liveToken += 1
        let token = liveToken
        liveParse = Task { @MainActor in
            var shown = ""
            while token == liveToken, liveText != shown {
                let text = liveText
                let blocks = await Task.detached(priority: .userInitiated) { AssistantMarkdown.blocks(text) }.value
                // The reply ended or another conversation was opened while this was read.
                guard token == liveToken else { return }
                liveBlocks = blocks
                shown = text
            }
            if token == liveToken { liveParse = nil }
        }
    }

    private func clearStream() {
        streamPublish?.cancel()
        streamPublish = nil
        replyPending = false
        stream.clear()
        showLive("")
    }

    // MARK: Leaving the app

    /// The app went to the background. While a reply is being written, asks iOS for time to
    /// finish it, as sync does for its last attempt, and returns true: events are still read
    /// for that time. iOS can end it whenever it likes; the conversation is read back on return.
    func enterBackground() -> Bool {
        inBackground = true
        guard running else { return false }
        if backgroundID == .invalid {
            backgroundID = UIApplication.shared.beginBackgroundTask(withName: "Finish assistant reply") { [weak self] in
                self?.endBackgroundTime()
            }
        }
        return backgroundID != .invalid
    }

    /// The app is in front again: the conversation is read back with whatever was written
    /// meanwhile. A reply that was cut off offers Continue interrupted reply.
    func enterForeground() {
        inBackground = false
        endBackgroundTime()
        if connected && !threadId.isEmpty { reloadThread() }
    }

    private func endBackgroundTime() {
        guard backgroundID != .invalid else { return }
        // Nothing reads events in the background once the reply is over.
        if inBackground { HostEvents.shared.stop() }
        UIApplication.shared.endBackgroundTask(backgroundID)
        backgroundID = .invalid
    }

    // MARK: Conversations

    func dismissError() { error = nil }
    func dismissNotice() { notice = nil }

    func newChat() {
        keepDraft()
        generation += 1
        conversation += 1
        clearStream()
        presentation = nil
        thread = nil
        entries = []
        setRunning(false)
        dropAttachments()
        thinking = ""
        undoProposal = nil
        undoToken = nil
        bringDraft()
    }

    func openThread(_ id: String) {
        operation("open") { [self] in
            keepDraft()
            generation += 1
            conversation += 1
            clearStream()
            presentation = nil
            thread = ["id": .string(id)]
            entries = []
            setRunning(false)
            thinking = ""
            dropAttachments()
            undoProposal = nil
            undoToken = nil
            bringDraft()
            try await loadThread(id)
        }
    }

    func deleteThread(_ id: String) {
        operation("delete") { [self] in
            _ = try await call("assistant_delete_thread", ["id": .string(id)])
            drafts[id] = nil
            if threadId == id {
                clearDraft()
                newChat()
            }
            try await loadHistory()
        }
    }

    func deleteAll() {
        operation("delete") { [self] in
            _ = try await call("assistant_delete_all")
            drafts = [:]
            if !threadId.isEmpty { clearDraft() }
            newChat()
            try await loadHistory()
        }
    }

    /// Gives a conversation another title: the open one from its menu, any from the history list.
    func renameThread(_ id: String, to title: String) {
        operation("rename") { [self] in
            _ = try await call("assistant_rename_thread", ["id": .string(id), "title": .string(title)])
            try await loadHistory()
            if threadId == id { try await loadThread(id) }
        }
    }

    /// Carries a conversation that has grown long on in a new one, which begins with what was
    /// said last in this one, and opens it as the history list opens one.
    func continueInNew() {
        operation("continue") { [self] in
            let id = try await call("assistant_continue", ["threadId": .string(threadId)]).string
            try? await loadHistory()
            openThread(id)
        }
    }

    // MARK: The message being written

    /// Adds text to the message being written, on a line of its own when there's something
    /// there already: a request from another screen, a suggestion, or a card's Ask for changes.
    func addToDraft(_ text: String) { draft = AssistantDraft.adding(text, to: draft) }

    /// The transactions that go with the next message, as the picker or a × leaves them.
    func select(_ ids: [String]) {
        selected = ids
        selectionTotal = nil
    }

    func clearDraft() {
        draft = ""
        select([])
    }

    /// What another screen asks about (its words, its selected transactions, its file) goes to
    /// the message being written, in a conversation of its own unless the open one has nothing
    /// in it yet. A selection over 200 is cut to its first 200 here, with the count kept for the
    /// sentence by the field. The same request is taken once, and a file already uploaded for the
    /// message being written isn't uploaded again.
    func take(_ request: AssistantRequest) {
        guard takenRequest != request.id else { return }
        takenRequest = request.id
        if !entries.isEmpty || running { newChat() }
        if !request.selected.isEmpty {
            let capped = AssistantSelection.capped(request.selected)
            selected = capped.ids
            selectionTotal = capped.total
        }
        if !request.text.isEmpty { addToDraft(request.text) }
        let files = request.files.filter { !requestFiles.contains($0) }
        if !files.isEmpty { attach(files: files, fromRequest: true) }
    }

    /// Keeps the message being written with the conversation being left.
    private func keepDraft() {
        let kept = AssistantDraft(text: draft, selected: selected, total: selectionTotal)
        drafts[threadId] = kept.isEmpty ? nil : kept
    }

    /// Brings back the message that was being written in the conversation now open.
    private func bringDraft() {
        let kept = drafts.removeValue(forKey: threadId) ?? AssistantDraft()
        draft = kept.text
        selected = kept.selected
        selectionTotal = kept.total
    }

    // MARK: Settings

    /// `done` runs once the change is saved and the status read again.
    func settings(_ change: JSON, done: (@MainActor () -> Void)? = nil) {
        operation("settings") { [self] in
            _ = try await call("assistant_settings", ["change": change])
            try await loadStatus()
            done?()
        }
    }

    func refreshModels() { operation("models") { [self] in try await loadModels(refresh: true) } }

    /// Starts ChatGPT sign-in. The service asks the app to show ChatGPT's page (host://open-url)
    /// and waits on its one-time 127.0.0.1 listener until the page redirects back or it's cancelled.
    /// `done` runs only when it signed in.
    func signIn(done: (@MainActor () -> Void)? = nil) {
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
                done?()
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

    /// Files chosen in Files, read and sent to the service one by one. One that came with
    /// another screen's request is remembered once it's up, so asking again doesn't upload it twice.
    func attach(files: [URL], fromRequest: Bool = false) {
        operation("attach") { [self] in
            let generation = generation
            for url in files {
                guard attachments.count < AssistantAttachments.limit else { throw AppError(String(localized: "Attach up to five files per message.")) }
                let file = try await Task.detached(priority: .userInitiated) { () throws -> (name: String, data: Data) in
                    let read = try AssistantAttachments.read(url)
                    return AssistantAttachments.prepare(name: read.name, data: read.data)
                }.value
                try await upload(file.name, file.data, generation: generation)
                if fromRequest && generation == self.generation { requestFiles.insert(url) }
            }
        }
    }

    /// Photos from the library: formats the service reads stay as they are, others become JPEG.
    func attach(photos: [PhotosPickerItem]) {
        operation("attach") { [self] in
            let generation = generation
            for item in photos {
                guard attachments.count < AssistantAttachments.limit else { throw AppError(String(localized: "Attach up to five files per message.")) }
                guard let data = try await item.loadTransferable(type: Data.self) else {
                    throw AppError(String(localized: "This photo couldn't be read. Choose another, or attach it as a file."))
                }
                guard data.count <= AssistantAttachments.maxBytes * 2 else { throw AppError(String(localized: "Choose a photo smaller than 50 MiB.")) }
                let type = item.supportedContentTypes.first { $0.conforms(to: .image) }
                let number = attachments.count + 1
                let file = try await Task.detached(priority: .userInitiated) {
                    try AssistantAttachments.photo(data, type: type, number: number)
                }.value
                guard file.data.count <= AssistantAttachments.maxBytes else { throw AppError(String(localized: "Choose a file smaller than 25 MiB.")) }
                try await upload(file.name, file.data, generation: generation)
            }
        }
    }

    private func upload(_ name: String, _ data: Data, generation: Int) async throws {
        let encoded = await Task.detached(priority: .userInitiated) { data.base64EncodedString() }.value
        let meta = try await call("assistant_attach", ["name": .string(name), "data": .string(encoded)])
        if generation == self.generation && attachments.count < AssistantAttachments.limit {
            attachments.append(meta)
        } else {
            // It went on no message: the conversation was left while the file was read.
            detach([meta.text("id")])
        }
    }

    func removeAttachment(_ id: String) {
        attachments.removeAll { $0.text("id") == id }
        // Taken off by hand: a request that brings the file again may attach it again.
        requestFiles = []
        // A message being sent takes its files with it; only one still waiting is forgotten.
        if !busy.contains("send") { detach([id]) }
    }

    /// Takes every file off the message being written, as starting or opening a conversation does.
    private func dropAttachments() {
        let ids = attachments.map { $0.text("id") }
        attachments = []
        requestFiles = []
        if !busy.contains("send") { detach(ids) }
    }

    /// Has the service forget files taken off a message before it was sent: they wait in its
    /// staging folder until Send. A failure isn't shown; the service clears that folder itself
    /// in a later session.
    private func detach(_ ids: [String]) {
        let ids = ids.filter { !$0.isEmpty }
        guard !ids.isEmpty else { return }
        Task { @MainActor in
            for id in ids { _ = try? await call("assistant_detach", ["id": .string(id)]) }
        }
    }

    // MARK: Messages

    /// Sends the message being written with what goes with it (`screen`, from `AssistantWith`).
    /// Once it's sent, the field keeps only what was typed since and the selection is used up;
    /// `done` then lets the screen start the next message with everything on it again.
    func send(screen: JSON, done: @escaping @MainActor () -> Void) {
        let text = draft
        operation("send") { [self] in
            guard screen["selected"].array.count <= AssistantSelection.limit else {
                throw AppError(String(localized: "Select up to 200 transactions per message."))
            }
            let generation = generation
            clearStream()
            thinking = ""
            let params: JSON = ["threadId": threadId.isEmpty ? .null : .string(threadId), "text": .string(text),
                                "attachments": JSON(attachments.map { $0.text("id") }), "screen": screen]
            let result = try await call("assistant_send", params).string
            guard generation == self.generation else { return }
            if threadId != result { thread = ["id": .string(result)] }
            setRunning(true)
            attachments = []
            requestFiles = []
            if draft == text { draft = "" }
            select([])
            done()
            try await loadThread(result)
            try await loadHistory()
        }
    }

    /// Asks again after an error or an interrupted reply, and for the rest of a reply that was
    /// cut short, without a new message.
    func retry(screen: JSON) {
        operation("send") { [self] in
            let id = threadId
            _ = try await call("assistant_retry", ["threadId": .string(id), "screen": screen])
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
        card(proposal, "apply") { [self] in
            let id = threadId
            let outcome = try await call("assistant_apply_commit", proposalParams(id, proposal), changesBudget: true)
            if !outcome["patch"].isNull {
                var token = outcome["snapshot"].text("token")
                if token.isEmpty { token = try await call("database_status").text("token") }
                undoToken = token
                undoProposal = outcome["undoAvailable"].bool ? proposal : nil
                notice = String(localized: "Proposal applied")
                successes += 1
            } else {
                notice = String(localized: "Your budget changed. Review the refreshed proposal before applying it.")
            }
            try await loadThread(id)
        }
    }

    /// Undoes an applied proposal, only while it is still the budget's latest change.
    func undo(_ proposal: String) {
        card(proposal, "undo") { [self] in
            let id = threadId
            let token = try await call("database_status").text("token")
            guard token == undoToken && proposal == undoProposal else {
                throw AppError(String(localized: "Your budget changed after this proposal. Use the budget's undo history to review subsequent changes."))
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
        card(proposal, "revise") { [self] in
            let id = threadId
            var params = proposalParams(id, proposal)
            params["revision"] = revision
            _ = try await call("assistant_revise", params)
            try await loadThread(id)
        }
    }

    func discard(_ proposal: String) {
        card(proposal, "discard") { [self] in
            var params = proposalParams(threadId, proposal)
            params["status"] = "discarded"
            _ = try await call("assistant_mark", params)
            try await loadThread(threadId)
        }
    }

    /// The bank-file review imported a converted statement: its card is done.
    func imported(thread: String, proposal: String) {
        successes += 1
        card(proposal, "mark") { [self] in
            var params = proposalParams(thread, proposal)
            params["status"] = "applied"
            _ = try await call("assistant_mark", params)
            if threadId == thread { try await loadThread(thread) }
        }
    }

    func report(_ proposal: String, done: @escaping @MainActor (JSON) -> Void) {
        card(proposal, "report", label: "report") { [self] in done(try await call("assistant_report", proposalParams(threadId, proposal))) }
    }

    func importFile(_ proposal: String, done: @escaping @MainActor (JSON, String) -> Void) {
        card(proposal, "import", label: "import") { [self] in
            let thread = threadId
            done(try await call("assistant_import", proposalParams(thread, proposal)), thread)
        }
    }

    func saveImport(_ proposal: String, done: @escaping @MainActor (String) -> Void) {
        card(proposal, "export", label: "export") { [self] in done(try await call("assistant_save_import", proposalParams(threadId, proposal)).string) }
    }

    /// Writes the workbook (or a report's tables) in the budget's exports folder and returns its
    /// path. The app previews or shares the file itself, so `open` is never asked of the service.
    /// `opening` says which of the card's two buttons asked, for the one that spins.
    func spreadsheet(_ proposal: String, opening: Bool, done: @escaping @MainActor (String) -> Void) {
        card(proposal, opening ? "open" : "export", label: "export") { [self] in
            var params = proposalParams(threadId, proposal)
            params["open"] = false
            done(try await call("assistant_spreadsheet", params).string)
        }
    }

    /// Keeps the person's thumb on a reply (`up`), or takes it off (nil). `index` is the reply's
    /// place in the conversation, as its entry gives it. Thumbs stay on this device.
    func rate(_ index: Int64, up: Bool?) {
        operation("rate") { [self] in
            let id = threadId
            let thumb: JSON = up.map(JSON.bool) ?? .null
            _ = try await call("assistant_rate", ["threadId": .string(id), "index": .int(index), "up": thumb])
            try await loadThread(id)
        }
    }

    /// Sends only the excerpt and explanation the person approved to UrBudget. `done` gets the
    /// report's reference, for the form to keep on screen.
    func reportReply(text: String, reason: String, done: @escaping @MainActor (String) -> Void) {
        operation("reportReply") { [self] in
            done(try await call("assistant_report_reply", ["text": .string(text), "reason": .string(reason)]).string)
        }
    }
}
