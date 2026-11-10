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
    /// The person's choices on proposal cards that the service doesn't have yet, by the
    /// proposal's id (`ProposalChoices`): a card shows them at once, and they're sent in one
    /// call a moment after the last of them.
    private(set) var choices: [String: JSON] = [:]
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
    /// The notice that offers Undo, and the budget revision its change left. The notice offers
    /// it only while it's the one showing and the budget hasn't changed since.
    private(set) var undoNotice: (text: String, revision: String)?
    /// Counts applied proposals and finished imports, for a success haptic.
    private(set) var successes = 0

    var threadId: String { thread?.text("id") ?? "" }
    /// The card whose change the budget's Undo would take back, as the conversation's view
    /// names it; empty when it's none of this conversation's.
    var undoable: String { thread?.text("undoable") ?? "" }
    /// What's waiting in the budget that the assistant can help with, offered under the last
    /// reply: each is a button's words (`text`) and the message it sends (`prompt`).
    var next: [JSON] { thread?.rows("next") ?? [] }
    var ready: Bool { status?.flag("ready") == true }
    var provider: String { status?.text("provider", "chatgpt") ?? "chatgpt" }
    var signingIn: Bool { busy.contains("signIn") || status?.flag("signingIn") == true }
    /// The notice of what gets sent hasn't been read for the chosen service yet.
    var noticeDue: Bool { status?.flag("noticeDue") == true }

    @ObservationIgnored private weak var store: BudgetStore?
    @ObservationIgnored private var identity: String?
    @ObservationIgnored private var connected = false
    /// The wait before a card's choices are sent, and the call sending them, by proposal id.
    @ObservationIgnored private var choiceTimers: [String: Task<Void, Never>] = [:]
    @ObservationIgnored private var revising: [String: Task<Void, Error>] = [:]
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
            // Choices not yet sent were for the other budget's cards: they have nowhere to go here.
            dropChoices()
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

    /// Runs one action of a proposal card and marks that card while it runs: only its buttons
    /// wait, and only the one that asked spins, while other cards carry on. An action given a
    /// `label` (a report, an export) still runs one at a time across the conversation.
    private func card(_ proposal: String, _ action: String, label: String? = nil, _ work: @escaping @MainActor () async throws -> Void) {
        let label = label ?? "card:" + proposal
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
        case "replaced":
            // The assistant made a newer proposal in an earlier one's place. The earlier card's
            // new view takes its place at once (never a second card), and what was being chosen
            // on it has nothing left to change; the newer card comes as a proposal of its own.
            forgetChoices(event["proposal"].text("id"))
            place(event["proposal"])
            reloadThread()
        default:
            // Anything newer: the conversation shows it when it's next read.
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

    /// Says that the assistant's import review imported a statement, as the app's own message
    /// does after a bank file's import: with Undo, while the import is still the budget's last
    /// change (`revision` is the budget's once it was saved).
    func sayImported(_ text: String, revision: String) {
        notice = text
        undoNotice = (text: text, revision: revision)
    }

    func newChat() {
        keepDraft()
        leaveChoices()
        generation += 1
        conversation += 1
        clearStream()
        presentation = nil
        thread = nil
        entries = []
        setRunning(false)
        dropAttachments()
        thinking = ""
        bringDraft()
    }

    func openThread(_ id: String) {
        operation("open") { [self] in
            keepDraft()
            leaveChoices()
            generation += 1
            conversation += 1
            clearStream()
            presentation = nil
            thread = ["id": .string(id)]
            entries = []
            setRunning(false)
            thinking = ""
            dropAttachments()
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
                // Its cards are gone with it, and what was being chosen on them.
                dropChoices()
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
            dropChoices()
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

    /// Saves the budget's notes for the assistant, always as the whole list: one change to the
    /// budget, which is read again with it as an applied proposal's is. `done` gets nil once
    /// it's saved, or why it wasn't (the service says when there are too many).
    func saveNotes(_ notes: [String], done: @escaping @MainActor (String?) -> Void) {
        guard !busy.contains("notes") else { return }
        busy.insert("notes")
        Task { @MainActor in
            defer { busy.remove("notes") }
            do {
                _ = try await call("assistant_notes_commit", ["notes": JSON(notes)], changesBudget: true)
                done(nil)
            } catch {
                done(Self.message(error))
            }
        }
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
    /// With `preset`, those words are sent for the person instead (a question's answer, a next
    /// step, a card asked for again): the message being written and its files stay as they are.
    func send(_ preset: String? = nil, screen: JSON, done: @escaping @MainActor () -> Void) {
        let text = preset ?? draft
        operation("send") { [self] in
            guard screen["selected"].array.count <= AssistantSelection.limit else {
                throw AppError(String(localized: "Select up to 200 transactions per message."))
            }
            let generation = generation
            // The assistant reads the cards as the person left them: choices still waiting go first.
            for proposal in Array(choices.keys) { try? await settleChoices(proposal) }
            guard generation == self.generation else { return }
            clearStream()
            thinking = ""
            let files = preset == nil ? attachments.map { $0.text("id") } : []
            let params: JSON = ["threadId": threadId.isEmpty ? .null : .string(threadId), "text": .string(text),
                                "attachments": JSON(files), "screen": screen]
            let result = try await call("assistant_send", params).string
            guard generation == self.generation else { return }
            if threadId != result { thread = ["id": .string(result)] }
            setRunning(true)
            if preset == nil {
                attachments = []
                requestFiles = []
                if draft == text { draft = "" }
            }
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

    /// A proposal card of the open conversation, as the service last showed it.
    private func shownCard(_ proposal: String) -> JSON? {
        entries.first { $0.kind == "proposal" && $0.json["proposal"].text("id") == proposal }?.json["proposal"]
    }

    /// Puts a card's newer view (from an event, or from a call that returns the card) in the
    /// open conversation in place of the one it shows: the card changes where it is.
    private func place(_ proposal: JSON) {
        let id = proposal.text("id")
        guard proposal.isObject, !id.isEmpty, var shown = thread else { return }
        var rows = shown["entries"].array
        guard let index = rows.firstIndex(where: { $0.text("kind") == "proposal" && $0["proposal"].text("id") == id }) else { return }
        rows[index]["proposal"] = proposal
        shown["entries"] = .array(rows)
        let prepared = AssistantThreadPresentation.make(shown, previous: presentation)
        presentation = prepared
        thread = prepared.thread
        entries = prepared.entries
    }

    /// The budget changed while the conversation is on screen. A change that waits is made
    /// again on it when the conversation is read, and the budget's own Undo and Redo move an
    /// applied card between applied and undone: with such a card, the conversation is read again.
    func budgetChanged() {
        guard entries.contains(where: { $0.kind == "proposal" && AssistantCard.followsBudget($0.json["proposal"]) }) else { return }
        reloadThread()
    }

    /// Rechecks the proposal against the budget as it is now, then commits it as one change:
    /// the service saves the budget first and marks the card applied after. Choices still
    /// waiting on the card go to the service first. A proposal the budget has moved past comes
    /// back changed instead, and its card says why: the card is the one place that's said.
    func apply(_ proposal: String) {
        card(proposal, "apply") { [self] in
            let id = threadId
            try await settleChoices(proposal)
            let outcome: JSON
            do {
                outcome = try await call("assistant_apply_commit", proposalParams(id, proposal), changesBudget: true)
            } catch {
                // Where the card stands is read again before the error shows.
                if threadId == id { try? await loadThread(id) }
                throw error
            }
            guard threadId == id else { return }
            place(outcome["proposal"])
            if !outcome["patch"].isNull {
                notice = String(localized: "Proposal applied")
                successes += 1
            }
            try await loadThread(id)
        }
    }

    /// Takes an applied proposal's change back. The service allows it only while that change is
    /// still the budget's latest, and says so when it isn't; the budget's revision goes with the
    /// call as `expectedToken` (`BudgetStore.service`).
    func undo(_ proposal: String) {
        card(proposal, "undo") { [self] in
            let id = threadId
            do {
                let outcome = try await call("assistant_undo_commit", proposalParams(id, proposal), changesBudget: true)
                if threadId == id { place(outcome["proposal"]) }
            } catch {
                // Whether it can still be undone is read again before the error shows.
                if threadId == id { try? await loadThread(id) }
                throw error
            }
            if threadId == id { try await loadThread(id) }
        }
    }

    /// Records where a card stands (`assistant_mark`) and shows the card as the service has it
    /// then. A step that can't follow from where the card stands is refused: the conversation
    /// is read again, and the service's reason shows.
    private func mark(_ thread: String, _ proposal: String, _ status: String, note: String? = nil) async throws {
        do {
            var params = proposalParams(thread, proposal)
            params["status"] = .string(status)
            if let note { params["note"] = .string(note) }
            let marked = try await call("assistant_mark", params)
            if threadId == thread { place(marked) }
        } catch {
            if threadId == thread { try? await loadThread(thread) }
            throw error
        }
        if threadId == thread { try await loadThread(thread) }
    }

    /// Sets a card aside. A thing the assistant made (a report, a spreadsheet, a converted
    /// file, figures) is removed the same way.
    func discard(_ proposal: String) {
        card(proposal, "discard") { [self] in
            let id = threadId
            // What was being chosen on it has nothing left to change; a call already sending
            // some of it finishes first, so it isn't refused by a card that's gone.
            forgetChoices(proposal)
            _ = try? await revising[proposal]?.value
            try await mark(id, proposal, "discarded")
        }
    }

    /// Puts a discarded, removed or replaced card back to wait again.
    func restore(_ proposal: String) {
        card(proposal, "restore") { [self] in try await mark(threadId, proposal, "proposed") }
    }

    /// The bank-file review imported a converted statement, or an import applied a card's
    /// fixes: its card is done. `note` says which of the fixes didn't take, when some didn't.
    func imported(thread: String, proposal: String, note: String? = nil) {
        successes += 1
        card(proposal, "mark") { [self] in try await mark(thread, proposal, "applied", note: note) }
    }

    /// A `fix` card as the service has it once the choices still waiting on it are sent: what
    /// its import applies. `done` gets the conversation's id and the card.
    func whenSettled(_ proposal: String, done: @escaping @MainActor (String, JSON) -> Void) {
        card(proposal, "apply") { [self] in
            let id = threadId
            try await settleChoices(proposal)
            guard threadId == id, let settled = shownCard(proposal) else { return }
            done(id, settled)
        }
    }

    // MARK: Choices on a card

    /// The person's choices on a card as they stand: those still waiting to be sent, else the
    /// ones the service made the card with.
    func choicesNow(_ proposal: String) -> JSON {
        choices[proposal] ?? shownCard(proposal)?["choices"] ?? .null
    }

    /// Keeps the person's choices on a card (`ProposalChoices`: rows left out, categories or
    /// amounts given). The card shows them at once. They go to the service in one call, about
    /// 600 ms after the last of them or at once with `now`, and always before Apply.
    func choose(_ proposal: String, _ revision: JSON, now: Bool = false) {
        guard !threadId.isEmpty, cardWork[proposal] == nil else { return }
        choices[proposal] = revision
        choiceTimers[proposal]?.cancel()
        choiceTimers[proposal] = Task { @MainActor in
            if !now { try? await Task.sleep(for: .milliseconds(600)) }
            // Another choice was made meanwhile: its own wait sends them all.
            guard !Task.isCancelled else { return }
            choiceTimers[proposal] = nil
            do {
                try await settleChoices(proposal)
            } catch {
                if !(error is CancellationError) { self.error = Self.message(error) }
            }
        }
    }

    /// Has the service make a card again with every choice made on it so far, and waits for
    /// that: what Apply does first, and the wait after the last choice.
    private func settleChoices(_ proposal: String) async throws {
        choiceTimers[proposal]?.cancel()
        choiceTimers[proposal] = nil
        // A call on its way took the choices as they were then; those made since go in another.
        while let sending = sendChoices(proposal) { try await sending.value }
    }

    /// The call sending a card's waiting choices: the one on its way, or a new one. There is
    /// one `assistant_revise` per card at a time. Nil when the service has every choice.
    private func sendChoices(_ proposal: String) -> Task<Void, Error>? {
        if let running = revising[proposal] { return running }
        guard let revision = choices[proposal], !threadId.isEmpty else { return nil }
        let id = threadId
        let sending = Task { @MainActor () async throws -> Void in
            // Cleared before anyone waiting on this call carries on, so they never find it again.
            defer { revising[proposal] = nil }
            do {
                var params = proposalParams(id, proposal)
                params["revision"] = revision
                let revised = try await call("assistant_revise", params)
                // The conversation was left meanwhile, and its waiting choices with it.
                guard threadId == id else { return }
                place(revised)
                // Choices made while it ran stay, for the next call.
                if choices[proposal] == revision { choices[proposal] = nil }
            } catch {
                // The card goes back to what the service has of it.
                if threadId == id { choices[proposal] = nil }
                throw error
            }
        }
        revising[proposal] = sending
        return sending
    }

    /// Forgets the choices waiting on a card, unsent.
    private func forgetChoices(_ proposal: String) {
        choiceTimers[proposal]?.cancel()
        choiceTimers[proposal] = nil
        if choices[proposal] != nil { choices[proposal] = nil }
    }

    /// Forgets every card's waiting choices, unsent: another budget's, or a deleted conversation's.
    private func dropChoices() {
        for timer in choiceTimers.values { timer.cancel() }
        choiceTimers = [:]
        if !choices.isEmpty { choices = [:] }
    }

    /// The conversation is being left. The choices still waiting on its cards go to the service
    /// as they are, so its cards have them when it's opened again; a failure isn't shown.
    private func leaveChoices() {
        let id = threadId
        let waiting = choices
        dropChoices()
        guard !id.isEmpty else { return }
        for (proposal, revision) in waiting {
            Task { @MainActor in
                var params = proposalParams(id, proposal)
                params["revision"] = revision
                _ = try? await call("assistant_revise", params)
            }
        }
    }

    // MARK: A card's files

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

    /// Sends UrBudget only what the person left in the report's fields: the reply's text, and
    /// their explanation with the details under it. `done` gets the report's reference, for the
    /// form to keep on screen.
    func reportReply(text: String, reason: String, done: @escaping @MainActor (String) -> Void) {
        operation("reportReply") { [self] in
            done(try await call("assistant_report_reply", ["text": .string(text), "reason": .string(reason)]).string)
        }
    }
}
