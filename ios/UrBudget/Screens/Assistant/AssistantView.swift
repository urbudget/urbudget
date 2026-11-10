import Observation
import PhotosUI
import QuickLook
import SwiftUI
import UIKit
import UniformTypeIdentifiers

/// A report, import or reply the chat opens in a sheet of its own.
struct AssistantSheetItem<Value>: Identifiable {
    let id = UUID()
    var value: Value
}

/// The import card's converted statement, for the bank-file review.
struct AssistantImportRequest {
    var file: JSON
    var threadId: String
    var proposalId: String
}

/// What another screen asks the assistant about: a question to send and transactions to discuss,
/// put in the composer for the person to send, as the desktop's ask() does. It says where it was
/// asked from, which goes with the messages sent from this sheet.
struct AssistantRequest: Identifiable {
    let id = UUID()
    /// The page it was asked from, by the key the service knows it by ("budget", "transactions",
    /// "accounts", "import"…); "assistant" when it's opened from nowhere in particular.
    var page = "assistant"
    /// The account in view there, by id: an account's register, or the account a file is imported into.
    var account = ""
    /// The category in view there, by id: the category's sheet.
    var category = ""
    var text = ""
    var selected: [String] = []
    /// Files to attach, such as a PDF statement chosen to import.
    var files: [URL] = []
    /// The bank file being imported when the assistant is opened over its import ({account,
    /// content, session}): the assistant is told its lines.
    var importing: JSON?
    /// Applies fixes the assistant proposes for that import, from their card (its thread, the proposal).
    var applyFix: ((String, JSON) -> Void)?
}

/// A reply to report: the excerpt to edit down, and the tools its turn had called, for the
/// details offered with the report.
struct AssistantReplyReport {
    var excerpt: String
    var tools: [String]
}

/// The top and bottom of the conversation's content, in its scroll view's coordinates.
struct AssistantScrollExtent: Equatable, Sendable {
    var top: CGFloat
    var bottom: CGFloat
}

/// Whether the conversation follows new text (`AssistantFollow`). A class of its own, so
/// scrolling redraws only the Jump to latest button and never the conversation.
@MainActor @Observable
final class AssistantScroll {
    /// The end of the conversation is on screen, so new text keeps it there.
    private(set) var following = true
    @ObservationIgnored private var viewport: CGFloat = 0
    /// Where the content's top rests while following: a scroll up is measured from it.
    @ObservationIgnored private var restingTop = CGFloat.greatestFiniteMagnitude

    /// The content moved under the finger, or grew.
    func moved(_ extent: AssistantScrollExtent) {
        let next = AssistantFollow.next(following, top: extent.top, restingTop: restingTop, bottom: extent.bottom, viewport: viewport)
        restingTop = AssistantFollow.resting(restingTop, top: extent.top, bottom: extent.bottom, viewport: viewport)
        if next != following { following = next }
    }

    /// The scroll view's height changed: the keyboard, or a longer message in the field.
    func resized(_ height: CGFloat) { viewport = height }

    /// Another conversation, a message sent, or Jump to latest: at the end, following again.
    func follow() {
        restingTop = .greatestFiniteMagnitude
        if !following { following = true }
    }
}

/// The assistant: a conversation about the budget whose changes arrive as proposals to review,
/// apply and undo. Presented as a large sheet with its own navigation stack.
struct AssistantView: View {
    @Environment(BudgetStore.self) private var store
    @Environment(\.dismiss) private var dismiss
    @Environment(\.scenePhase) private var phase
    private let controller = AssistantController.shared

    @State private var settingsShown = false
    @State private var historyShown = false
    @State private var contextShown = false
    /// What waits on the notice of what gets sent: a message, or a reply asked for again.
    @State private var afterNotice: AssistantSheetItem<() -> Void>?
    @State private var choosingFiles = false
    @State private var choosingPhotos = false
    @State private var photoItems: [PhotosPickerItem] = []
    @State private var report: AssistantSheetItem<JSON>?
    @State private var importReview: AssistantSheetItem<AssistantImportRequest>?
    @State private var replyReport: AssistantSheetItem<AssistantReplyReport>?
    @State private var share: AssistantSheetItem<URL>?
    @State private var changing: AssistantSheetItem<AssistantRowChange>?
    @State private var preview: URL?
    @State private var localError: String?
    @State private var sent = 0
    /// Parts of "With this message" taken off the message being written.
    @State private var placeOff = false
    @State private var importOff = false
    @State private var renaming = false
    @State private var newTitle = ""
    @State private var scroll = AssistantScroll()
    @FocusState private var composing: Bool
    @ScaledMetric(relativeTo: .body) private var iconSize: CGFloat = 36

    private let request: AssistantRequest?

    init(request: AssistantRequest? = nil) { self.request = request }

    private var sheetShown: Bool {
        historyShown || contextShown || afterNotice != nil || report != nil || importReview != nil || replyReport != nil || share != nil || preview != nil || changing != nil
    }

    /// The open budget's currency, for the amounts on cards and in an exported conversation.
    private var currency: String { store.budget?.text("currency") ?? "" }

    /// What goes with the next message: where this sheet was opened from, the month the app is
    /// on, the selected transactions and the file being imported, less what was taken off.
    private var with: AssistantWith {
        AssistantWith(page: request?.page ?? "assistant", month: store.month, account: request?.account ?? "", category: request?.category ?? "",
                      selected: controller.selected, importing: request?.importing, placeOff: placeOff, importOff: importOff)
    }

    var body: some View {
        NavigationStack {
            conversation
                .background(Color.groupedBackground)
                .safeAreaInset(edge: .top, spacing: 0) { noticeBar }
                .safeAreaInset(edge: .bottom, spacing: 0) { composer }
                .navigationTitle("Assistant")
                .inlineTitle()
                .toolbar { toolbar }
                .navigationDestination(isPresented: $settingsShown) { AssistantSettingsView() }
                .alert("Rename conversation", isPresented: $renaming) {
                    TextField("Conversation name", text: $newTitle)
                    Button("Save") {
                        let title = newTitle.trimmingCharacters(in: .whitespacesAndNewlines)
                        if !title.isEmpty && !controller.threadId.isEmpty { controller.renameThread(controller.threadId, to: title) }
                    }
                    Button("Cancel", role: .cancel) {}
                }
        }
        .task {
            controller.connect(store)
            controller.refresh()
            if let request {
                controller.take(request)
                composing = true
            }
        }
        .task {
            // While a reply runs, catch up on anything the event stream missed.
            while !Task.isCancelled {
                try? await Task.sleep(for: .milliseconds(1200))
                if controller.running { controller.reloadThread() }
            }
        }
        // The controller follows the services' events itself; here it's only told whether the
        // assistant is on screen, for the dot that says something arrived while it wasn't.
        .onAppear { controller.shown(true) }
        .onDisappear { controller.shown(false) }
        .onChange(of: phase) { _, phase in if phase == .active { controller.refresh() } }
        .onChange(of: store.identity) { _, _ in
            controller.connect(store)
            controller.refresh()
        }
        .sensoryFeedback(.impact(weight: .light), trigger: sent)
        .sensoryFeedback(.success, trigger: controller.successes)
        .fileImporter(isPresented: $choosingFiles, allowedContentTypes: AssistantAttachments.contentTypes,
                      allowsMultipleSelection: true) { result in
            switch result {
            case .success(let urls):
                let room = max(0, AssistantAttachments.limit - controller.attachments.count)
                if urls.count > room { localError = String(localized: "Attach up to five files per message.") }
                controller.attach(files: Array(urls.prefix(room)))
            case .failure(let error):
                localError = BudgetStore.readable(error)
            }
        }
        .photosPicker(isPresented: $choosingPhotos, selection: $photoItems,
                      maxSelectionCount: max(1, AssistantAttachments.limit - controller.attachments.count), matching: .images)
        .onChange(of: photoItems) { _, items in
            guard !items.isEmpty else { return }
            controller.attach(photos: items)
            photoItems = []
        }
        .sheet(isPresented: $historyShown) { AssistantHistorySheet() }
        .sheet(isPresented: $contextShown) {
            AssistantContextPicker(selected: Binding(get: { controller.selected }, set: { controller.select($0) }))
        }
        .sheet(item: $afterNotice) { waiting in
            AssistantSendNotice {
                afterNotice = nil
                waiting.value()
            }
        }
        .sheet(item: $report) { AssistantReportView(report: $0.value) }
        .sheet(item: $importReview) { item in
            AssistantImportReview(request: item.value) {
                controller.imported(thread: item.value.threadId, proposal: item.value.proposalId)
            }
        }
        .sheet(item: $replyReport) { AssistantReplyReportForm(excerpt: $0.value.excerpt, tools: $0.value.tools) }
        .sheet(item: $changing) { item in
            AssistantRowEditor(row: item.value.row) { change in
                let proposal = item.value.proposal
                controller.revise(proposal.text("id"), ProposalChoices.change(proposal["choices"], key: item.value.row.text("key"), change: change))
            }
        }
        .sheet(item: $share) { AssistantShareSheet(items: [$0.value]) { share = nil }.presentationDetents([.medium, .large]) }
        .quickLookPreview($preview)
        .alert("Couldn’t complete that", isPresented: Binding(get: {
            (controller.error != nil || localError != nil) && !settingsShown && !sheetShown
        }, set: { shown in
            if !shown { controller.dismissError(); localError = nil }
        })) {
            Button("OK", role: .cancel) {}
        } message: {
            Text(localError ?? controller.error ?? "")
        }
    }

    // MARK: Conversation

    /// The conversation, following new text only while its end is on screen. Where it is
    /// scrolled to is measured from the content's own frame (`AssistantScroll`), not from a row
    /// at the end appearing, which a lazy stack can report for a row that's off screen.
    private var conversation: some View {
        ScrollViewReader { proxy in
            ScrollView {
                LazyVStack(alignment: .leading, spacing: 0) {
                    if controller.entries.isEmpty && !controller.running && controller.threadId.isEmpty {
                        emptyState.padding(.vertical, 12)
                    }
                    if let from = controller.thread?.text("continuedFrom"), !from.isEmpty {
                        Label("Continued from “\(from)”", systemImage: "arrow.turn.down.right")
                            .font(.footnote)
                            .foregroundStyle(Color.secondary)
                            .padding(.vertical, 7)
                    }
                    ForEach(controller.entries) { entry in
                        // A kind this version doesn't know takes no room at all.
                        if Self.kinds.contains(entry.kind) { entryView(entry).padding(.vertical, 7) }
                    }
                    AssistantLiveReply(controller: controller, scroll: scroll, proxy: proxy)
                    if controller.running || controller.busy.contains("send") {
                        AssistantWorkingRow(controller: controller)
                    } else if controller.entries.last?.kind == "user" {
                        Button("Continue interrupted reply", systemImage: "arrow.clockwise") { retry() }
                            .font(.subheadline)
                            .disabled(controller.busy.contains("send"))
                            .padding(.vertical, 8)
                    }
                    Color.clear.frame(height: 1).id("bottom")
                }
                .padding(.horizontal, 16)
                .padding(.vertical, 8)
                .onGeometryChange(for: AssistantScrollExtent.self) { geometry in
                    let frame = geometry.frame(in: .named("assistantScroll"))
                    return AssistantScrollExtent(top: frame.minY, bottom: frame.maxY)
                } action: { extent in
                    scroll.moved(extent)
                }
            }
            // A new conversation keeps this scroll view when its first message gives it an id.
            .id(controller.conversation)
            .coordinateSpace(.named("assistantScroll"))
            .defaultScrollAnchor(.bottom)
            .scrollDismissesKeyboard(.interactively)
            .onGeometryChange(for: CGFloat.self) { geometry in
                geometry.size.height
            } action: { height in
                scroll.resized(height)
                // The keyboard, or a longer message in the field, doesn't cover the end.
                if scroll.following { proxy.scrollTo("bottom", anchor: .bottom) }
            }
            .overlay(alignment: .bottom) {
                AssistantJumpButton(scroll: scroll) {
                    scroll.follow()
                    withAnimation(.easeOut(duration: 0.25)) { proxy.scrollTo("bottom", anchor: .bottom) }
                }
            }
            .onChange(of: controller.conversation) { _, _ in scroll.follow() }
            .onChange(of: controller.entries.count) { _, count in
                if scroll.following || count < 4 { withAnimation(.easeOut(duration: 0.25)) { proxy.scrollTo("bottom", anchor: .bottom) } }
            }
            .onChange(of: controller.running) { _, _ in
                if scroll.following { withAnimation(.easeOut(duration: 0.25)) { proxy.scrollTo("bottom", anchor: .bottom) } }
            }
            .onChange(of: sent) { _, _ in
                scroll.follow()
                withAnimation { proxy.scrollTo("bottom", anchor: .bottom) }
            }
        }
    }

    private var emptyState: some View {
        VStack(alignment: .leading, spacing: 12) {
            Image(systemName: "sparkles").font(.largeTitle).foregroundStyle(Color.urBrand).accessibilityHidden(true)
            Text("Ask about your budget. Review every change.").font(.title2.bold())
            Text("Look up spending, cover overspending, categorize selected transactions, read statements, or create a report. Proposed changes are checked again before you apply them.")
                .font(.subheadline).foregroundStyle(Color.secondary)
            if controller.status == nil {
                if controller.statusFailed {
                    Text("The assistant couldn't be loaded.").font(.subheadline)
                    Button("Try again") { controller.refresh() }
                        .buttonStyle(.bordered)
                } else {
                    HStack(spacing: 8) {
                        ProgressView().controlSize(.small)
                        Text("Checking the assistant…").font(.footnote).foregroundStyle(Color.secondary)
                    }
                    .padding(.top, 4)
                    .accessibilityElement(children: .combine)
                }
            } else if !controller.ready {
                Button("Set up assistant") { settingsShown = true }
                    .buttonStyle(.borderedProminent)
                    .padding(.top, 4)
            } else {
                VStack(spacing: 8) {
                    ForEach(Self.suggestions, id: \.self) { prompt in
                        Button {
                            controller.addToDraft(prompt)
                            composing = true
                        } label: {
                            HStack {
                                Text(prompt).multilineTextAlignment(.leading)
                                Spacer(minLength: 8)
                                Image(systemName: "arrow.up.backward").font(.footnote).foregroundStyle(Color.secondary)
                            }
                            .padding(.horizontal, 14)
                            .padding(.vertical, 12)
                            .frame(maxWidth: .infinity)
                            .background(Color.cardBackground, in: RoundedRectangle(cornerRadius: 12, style: .continuous))
                        }
                        .buttonStyle(.plain)
                        .accessibilityHint("Puts this question in the message field")
                    }
                }
                .padding(.top, 4)
            }
        }
        .frame(maxWidth: .infinity, alignment: .leading)
    }

    /// Questions to start with, put in the message field in the app's language.
    private static let suggestions = [String(localized: "Where is my money going this month?"), String(localized: "Help me cover overspending"),
                                      String(localized: "Make a report of this month's budget")]

    /// The kinds of entry this version draws.
    private static let kinds: Set<String> = ["user", "assistant", "step", "proposal", "stopped", "error", "cut"]

    @ViewBuilder
    private func entryView(_ entry: AssistantEntry) -> some View {
        switch entry.kind {
        case "proposal":
            let proposal = entry.json["proposal"]
            if proposal.isObject {
                // Its buttons wait while any card's change is made; only its own action spins.
                let working = controller.cardWork[proposal.text("id")]
                AssistantProposalCard(proposal: proposal, currency: currency,
                                      busy: controller.busy.contains("proposal") || working != nil,
                                      working: working,
                                      canUndo: controller.undoProposal == proposal.text("id"),
                                      actions: actions(for: proposal))
            }
        case "step":
            let failed = entry.json.flag("failed")
            Label {
                Text(failed ? String(localized: "Couldn’t complete: \(entry.json.text("text"))") : entry.json.text("text"))
            } icon: {
                Image(systemName: failed ? "exclamationmark.circle" : "checkmark.circle")
            }
            .font(.footnote)
            .foregroundStyle(failed ? Color.urNegative : .secondary)
        case "stopped":
            Label("Reply stopped", systemImage: "stop.circle").font(.footnote).foregroundStyle(Color.secondary)
        case "user":
            AssistantUserBubble(entry: entry.json)
        case "cut":
            VStack(alignment: .leading, spacing: 6) {
                Label("The reply was cut short.", systemImage: "scissors").font(.footnote).foregroundStyle(Color.secondary)
                // Only the conversation's last entry can be carried on from.
                if entry.id == controller.entries.last?.id && !controller.running {
                    Button("Continue") { retry() }
                        .font(.subheadline.weight(.semibold))
                        .disabled(controller.busy.contains("send"))
                }
            }
        case "error":
            VStack(alignment: .leading, spacing: 8) {
                Label("Couldn’t finish", systemImage: "exclamationmark.triangle.fill").font(.subheadline.weight(.semibold)).foregroundStyle(Color.urNegative)
                Text(entry.json.text("text")).font(.subheadline)
                // Only the conversation's last error says what helps; an earlier one has no button.
                if let action = AssistantErrorAction.of(entry.json) {
                    errorButton(action)
                        .font(.subheadline.weight(.semibold))
                        .disabled(controller.running || controller.busy.contains("send") || controller.signingIn)
                }
            }
            .padding(14)
            .frame(maxWidth: .infinity, alignment: .leading)
            .background(Color.urNegative.opacity(0.1), in: RoundedRectangle(cornerRadius: 14, style: .continuous))
        case "assistant":
            AssistantReply(text: entry.json.text("text"), blocks: entry.blocks, rating: entry.json.text("rating"), rate: rating(entry.json)) { excerpt in
                replyReport = AssistantSheetItem(value: AssistantReplyReport(excerpt: excerpt, tools: entry.json["tools"].array.map(\.string)))
            }
        default:
            // A kind this version doesn't know draws nothing, rather than an empty reply.
            EmptyView()
        }
    }

    /// Sets or takes off the person's thumb on a reply; nil for one the service gave no place
    /// in the conversation, which can't be rated.
    private func rating(_ entry: JSON) -> ((Bool?) -> Void)? {
        guard entry.has("index") else { return nil }
        let index = entry.long("index")
        return { up in controller.rate(index, up: up) }
    }

    /// The button under the conversation's last error, doing what the service says helps.
    @ViewBuilder
    private func errorButton(_ action: AssistantErrorAction) -> some View {
        switch action {
        case .retry:
            Button("Try again") { retry() }
        case .settings:
            Button("Assistant settings") { settingsShown = true }
        case .usage:
            Button("Plan usage") { controller.openLink("usage") }
        case .newConversation:
            Button("Continue in a new conversation") { controller.continueInNew() }
        case .signIn:
            Button("Continue with ChatGPT") {
                // Signed in, it asks again, unless another conversation was opened meanwhile.
                let thread = controller.threadId
                controller.signIn {
                    if controller.threadId == thread { retry() }
                }
            }
        }
    }

    private func actions(for proposal: JSON) -> AssistantProposalActions {
        let id = proposal.text("id")
        return AssistantProposalActions(
            apply: { controller.apply(id) },
            undo: { controller.undo(id) },
            discard: { controller.discard(id) },
            refine: {
                // Added under whatever is being written, never over it.
                controller.addToDraft(String(localized: "Revise the proposal ‘\(proposal.text("title"))’: "))
                composing = true
            },
            reviewImport: {
                controller.importFile(id) { file, thread in
                    importReview = AssistantSheetItem(value: AssistantImportRequest(file: file, threadId: thread, proposalId: id))
                }
            },
            saveImport: { controller.saveImport(id) { path in shareFile(path) } },
            report: { controller.report(id) { report = AssistantSheetItem(value: $0) } },
            spreadsheet: { open in
                controller.spreadsheet(id, opening: open) { path in
                    if open {
                        do { preview = try AssistantExport.savedFile(path) } catch { localError = BudgetStore.readable(error) }
                    } else {
                        shareFile(path)
                    }
                }
            },
            applyFix: request?.applyFix.map { fix -> () -> Void in { fix(controller.threadId, proposal) } },
            revise: { controller.revise(id, $0) },
            change: { changing = AssistantSheetItem(value: AssistantRowChange(proposal: proposal, row: $0)) })
    }

    private func shareFile(_ path: String) {
        do { share = AssistantSheetItem(value: try AssistantExport.savedFile(path)) } catch { localError = BudgetStore.readable(error) }
    }

    private func exportConversation() {
        guard let thread = controller.thread else { return }
        let currency = currency
        Task {
            do {
                let url = try await Task.detached(priority: .userInitiated) {
                    let text = AssistantExport.conversationMarkdown(thread, currency: currency)
                    return try AssistantExport.temporaryFile(named: AssistantExport.fileName(thread.text("title"), extension: "md"), data: Data(text.utf8))
                }.value
                share = AssistantSheetItem(value: url)
            } catch {
                localError = BudgetStore.readable(error)
            }
        }
    }

    // MARK: Bars

    @ToolbarContentBuilder
    private var toolbar: some ToolbarContent {
        ToolbarItem(placement: .cancellationAction) {
            Button("Done") { dismiss() }
        }
        ToolbarItem(placement: .principal) {
            VStack(spacing: 0) {
                HStack(spacing: 6) {
                    Text("Assistant").font(.headline)
                    if !controller.busy.subtracting(["send", "models", "status"]).isEmpty {
                        ProgressView().controlSize(.mini).accessibilityLabel("Loading")
                    }
                }
                Text(controller.thread?.text("title").nilIfEmpty ?? String(localized: "Your budget, with a second pair of eyes"))
                    .font(.caption2).foregroundStyle(Color.secondary).lineLimit(1)
            }
            .accessibilityElement(children: .combine)
        }
        ToolbarItemGroup(placement: .trailingBar) {
            Button { historyShown = true } label: { Image(systemName: "clock.arrow.circlepath") }
                .accessibilityLabel("Conversation history")
            Button {
                controller.newChat()
                controller.clearDraft()
                placeOff = false
                importOff = false
            } label: { Image(systemName: "square.and.pencil") }
                .accessibilityLabel("New conversation")
            Menu {
                Button("Rename conversation", systemImage: "pencil") {
                    newTitle = controller.thread?.text("title") ?? ""
                    renaming = true
                }
                .disabled(controller.threadId.isEmpty)
                Button("Export conversation", systemImage: "square.and.arrow.up") { exportConversation() }
                    .disabled(controller.thread == nil || controller.entries.isEmpty)
                Button("Assistant settings", systemImage: "gearshape") { settingsShown = true }
            } label: {
                Image(systemName: "ellipsis.circle")
            }
            .accessibilityLabel("More assistant actions")
        }
    }

    @ViewBuilder
    private var noticeBar: some View {
        if let notice = controller.notice {
            Notice(text: notice, tone: "info", icon: "info.circle.fill", action: String(localized: "Dismiss")) { controller.dismissNotice() }
                .padding(.horizontal, 16)
                .padding(.vertical, 6)
                .background(Color.groupedBackground)
                .transition(.move(edge: .top).combined(with: .opacity))
                .task(id: notice) {
                    try? await Task.sleep(for: .seconds(8))
                    if controller.notice == notice { controller.dismissNotice() }
                }
        }
    }

    private var canSend: Bool {
        controller.ready && !controller.busy.contains("send") && !controller.busy.contains("attach")
            && (!controller.draft.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty || !controller.attachments.isEmpty)
    }

    /// Runs `action` at once, or once the person has read the notice of what gets sent when it's due
    /// for the chosen service (the first message to it, or a reply asked for again after another
    /// was chosen): nothing goes to a service before that. Not now there leaves `action` undone.
    private func whenTold(_ action: @escaping () -> Void) {
        if controller.ready && controller.noticeDue {
            afterNotice = AssistantSheetItem(value: action)
        } else {
            action()
        }
    }

    private func send() {
        guard canSend else { return }
        whenTold {
            sent += 1
            controller.send(screen: with.screen) {
                // The next message starts with everything that goes with it again.
                placeOff = false
                importOff = false
            }
        }
    }

    /// Asks for the reply again, after an error or an interrupted reply, or for the rest of one
    /// that was cut short. The selected transactions wait for the next message.
    private func retry() {
        whenTold {
            var again = with
            again.selected = []
            controller.retry(screen: again.screen)
        }
    }

    /// Takes a part of "With this message" off the message being written.
    private func remove(_ part: AssistantWithPart.Kind) {
        switch part {
        case .place: placeOff = true
        case .selection: controller.select([])
        case .importing: importOff = true
        }
    }

    /// Above the field in a conversation that can't be carried on as it is: the assistant isn't
    /// ready any more (signed out, or the key removed), or the conversation has grown long.
    /// Each has the one action that helps.
    @ViewBuilder
    private var composerBanner: some View {
        if controller.status != nil && !controller.ready && !controller.entries.isEmpty {
            if controller.provider == "chatgpt" {
                AssistantBanner(text: String(localized: "Set up the assistant to send messages"), action: String(localized: "Continue with ChatGPT"),
                                busy: controller.signingIn) { controller.signIn() }
            } else {
                AssistantBanner(text: String(localized: "Set up the assistant to send messages"), action: String(localized: "Assistant settings")) {
                    settingsShown = true
                }
            }
        } else if controller.thread?.flag("long") == true && !controller.running {
            AssistantBanner(text: String(localized: "This conversation is getting long, so replies are slower and use more of your plan or key."),
                            action: String(localized: "Continue in a new conversation"), busy: controller.busy.contains("continue")) {
                controller.continueInNew()
            }
        }
    }

    /// What goes with the next message, each part with its own ×, and the sentence for a
    /// selection that was cut to its first 200.
    @ViewBuilder
    private var withLine: some View {
        let parts = with.parts(budget: store.budget)
        if !parts.isEmpty {
            VStack(alignment: .leading, spacing: 4) {
                Text("With this message").font(.caption).foregroundStyle(Color.secondary).padding(.horizontal, 12)
                ScrollView(.horizontal, showsIndicators: false) {
                    HStack(spacing: 8) {
                        ForEach(parts) { part in
                            withChip(part)
                        }
                    }
                    .padding(.horizontal, 12)
                }
            }
        }
        if let total = controller.selectionTotal {
            Text("Only the first 200 of \(total) selected transactions are attached.")
                .font(.caption)
                .foregroundStyle(Color.secondary)
                .padding(.horizontal, 12)
        }
    }

    /// One part of "With this message". The selected transactions open their picker when tapped.
    private func withChip(_ part: AssistantWithPart) -> some View {
        var tap: (() -> Void)?
        var removeLabel = String(localized: "Remove \(part.text)")
        if part.kind == .selection {
            tap = { contextShown = true }
            removeLabel = String(localized: "Clear selected transactions")
        }
        return AssistantChip(text: part.text, symbol: Self.symbol(part.kind), removeLabel: removeLabel, onTap: tap) {
            remove(part.kind)
        }
    }

    private static func symbol(_ part: AssistantWithPart.Kind) -> String {
        switch part {
        case .place: return "mappin.and.ellipse"
        case .selection: return "checklist"
        case .importing: return "doc.text"
        }
    }

    private var composer: some View {
        VStack(alignment: .leading, spacing: 8) {
            composerBanner
            if !controller.attachments.isEmpty || controller.busy.contains("attach") {
                ScrollView(.horizontal, showsIndicators: false) {
                    HStack(spacing: 8) {
                        ForEach(Array(controller.attachments.enumerated()), id: \.offset) { _, file in
                            AssistantChip(text: file.text("name"), symbol: AssistantAttachments.symbol(file.text("kind")),
                                          removeLabel: String(localized: "Remove attachment \(file.text("name"))")) {
                                controller.removeAttachment(file.text("id"))
                            }
                        }
                        if controller.busy.contains("attach") {
                            HStack(spacing: 6) {
                                ProgressView().controlSize(.small)
                                Text("Attaching…").font(.footnote).foregroundStyle(Color.secondary)
                            }
                            .padding(.horizontal, 10)
                        }
                    }
                    .padding(.horizontal, 12)
                }
            }
            withLine
            HStack(alignment: .bottom, spacing: 8) {
                Menu {
                    Button("Choose file", systemImage: "doc") { choosingFiles = true }
                    Button("Photo library", systemImage: "photo.on.rectangle") { choosingPhotos = true }
                } label: {
                    composerIcon("paperclip")
                }
                // Nothing is uploaded for a message that can't be sent.
                .disabled(!controller.ready || controller.running || controller.attachments.count >= AssistantAttachments.limit || controller.busy.contains("attach"))
                .accessibilityLabel("Attach statement, document, or image")
                Menu {
                    Button("Select transactions…", systemImage: "checklist") { contextShown = true }
                    if !controller.selected.isEmpty {
                        Button("Clear selected transactions", systemImage: "xmark.circle", role: .destructive) { controller.select([]) }
                    }
                } label: {
                    composerIcon(controller.selected.isEmpty ? "text.badge.plus" : "text.badge.checkmark")
                }
                .disabled(controller.running)
                .accessibilityLabel("Add budget context")
                TextField(controller.ready || controller.status == nil ? "Ask about your budget" : "Set up the assistant to send messages",
                          text: Binding(get: { controller.draft }, set: { controller.draft = $0.count > 20_000 ? String($0.prefix(20_000)) : $0 }),
                          axis: .vertical)
                    .lineLimit(1...6)
                    .focused($composing)
                    .padding(.horizontal, 14)
                    .padding(.vertical, 9)
                    .background(Color.cardBackground, in: RoundedRectangle(cornerRadius: 20, style: .continuous))
                    .overlay(RoundedRectangle(cornerRadius: 20, style: .continuous).strokeBorder(Color.secondary.opacity(0.25)))
                if controller.running {
                    Button { controller.stop() } label: {
                        Image(systemName: "stop.circle.fill").font(.title).foregroundStyle(Color.urAccent)
                    }
                    .disabled(controller.busy.contains("stop"))
                    .accessibilityLabel("Stop reply")
                } else {
                    Button(action: send) {
                        Image(systemName: "arrow.up.circle.fill").font(.title).foregroundStyle(canSend ? Color.urAccent : Color.secondary.opacity(0.5))
                    }
                    .disabled(!canSend)
                    .accessibilityLabel("Send message")
                }
            }
            .padding(.horizontal, 12)
        }
        .padding(.vertical, 8)
        .background(.bar)
    }

    private func composerIcon(_ symbol: String) -> some View {
        Image(systemName: symbol)
            .font(.body.weight(.semibold))
            .foregroundStyle(Color.urAccent)
            .frame(width: iconSize, height: iconSize)
            .background(Color.cardBackground, in: Circle())
    }
}

/// "Working…", or what the assistant is doing now while no text is arriving. A view of its own,
/// so a new activity redraws this row and not the conversation.
private struct AssistantWorkingRow: View {
    var controller: AssistantController

    var body: some View {
        let activity = controller.activity
        // What it's doing reads as under way: "Looking at October…".
        let doing = activity.isEmpty || activity.hasSuffix("…") ? activity : activity + "…"
        HStack(spacing: 8) {
            ProgressView().controlSize(.small)
            Text(doing.isEmpty ? String(localized: "Working…") : doing).font(.footnote).foregroundStyle(Color.secondary)
        }
        .padding(.vertical, 8)
        .accessibilityElement(children: .combine)
        .accessibilityLabel(doing.isEmpty ? String(localized: "The assistant is working") : doing)
    }
}

/// Jump to latest, over the end of the conversation once the person has scrolled up from it.
private struct AssistantJumpButton: View {
    var scroll: AssistantScroll
    var action: () -> Void

    var body: some View {
        if !scroll.following {
            Button(action: action) {
                Label("Jump to latest", systemImage: "arrow.down")
                    .font(.footnote.weight(.semibold))
                    .foregroundStyle(Color.urAccent)
                    .padding(.horizontal, 12)
                    .padding(.vertical, 8)
                    .background(.regularMaterial, in: Capsule())
                    .overlay(Capsule().strokeBorder(Color.secondary.opacity(0.25)))
            }
            .buttonStyle(.plain)
            .padding(.bottom, 8)
        }
    }
}

/// A notice above the message field, with the one action that helps.
private struct AssistantBanner: View {
    var text: String
    var action: String
    var busy = false
    var onAction: () -> Void

    var body: some View {
        VStack(alignment: .leading, spacing: 6) {
            Text(text).font(.footnote).fixedSize(horizontal: false, vertical: true)
            Button(action, action: onAction)
                .font(.footnote.weight(.semibold))
                .buttonStyle(.borderless)
                .disabled(busy)
        }
        .padding(.horizontal, 12)
        .padding(.vertical, 10)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(Color.urAccent.opacity(0.12), in: RoundedRectangle(cornerRadius: 12, style: .continuous))
        .padding(.horizontal, 12)
    }
}

/// Your message: a bubble at the trailing edge, with its attachments and what went with it under it.
private struct AssistantUserBubble: View {
    var entry: JSON

    var body: some View {
        let sentWith = AssistantWith.sent(entry["with"])
        HStack {
            Spacer(minLength: 48)
            VStack(alignment: .trailing, spacing: 6) {
                if !entry.text("text").isEmpty {
                    Text(entry.text("text"))
                        .foregroundStyle(.white)
                        .padding(.horizontal, 14)
                        .padding(.vertical, 10)
                        .background(Color.urAccent, in: RoundedRectangle(cornerRadius: 18, style: .continuous))
                        .contextMenu {
                            Button("Copy", systemImage: "doc.on.doc") { UIPasteboard.general.string = entry.text("text") }
                        }
                }
                ForEach(Array(entry.rows("attachments").enumerated()), id: \.offset) { _, file in
                    Label(file.text("name"), systemImage: AssistantAttachments.symbol(file.text("kind")))
                        .font(.footnote)
                        .lineLimit(1)
                        .padding(.horizontal, 10)
                        .padding(.vertical, 6)
                        .background(Color.cardBackground, in: Capsule())
                        .accessibilityLabel("Attachment \(file.text("name"))")
                }
                if let sentWith {
                    Text(sentWith).font(.caption).foregroundStyle(Color.secondary).multilineTextAlignment(.trailing)
                }
            }
        }
        .accessibilityElement(children: .combine)
        .accessibilityLabel("You: \(entry.text("text"))")
        .accessibilityValue(sentWith ?? "")
    }
}

/// The assistant's reply in Markdown, with copy, share, report and the two thumbs under it.
private struct AssistantReply: View {
    var text: String
    var blocks: [AssistantMarkdownBlock]
    /// The person's thumb on this reply: "up", "down", or nothing.
    var rating: String
    /// Sets the thumb (true is up), or takes it off (nil). Nil for a reply that can't be rated.
    var rate: ((Bool?) -> Void)?
    var report: (String) -> Void
    @State private var copied = 0

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            AssistantMarkdownView(blocks: blocks)
            HStack(spacing: 18) {
                Button {
                    UIPasteboard.general.string = text
                    copied += 1
                } label: { Image(systemName: "doc.on.doc") }
                    .accessibilityLabel("Copy reply")
                ShareLink(item: text) { Image(systemName: "square.and.arrow.up") }
                    .accessibilityLabel("Share reply")
                Button { report(String(text.prefix(4000))) } label: { Image(systemName: "flag") }
                    .accessibilityLabel("Report reply")
                if let rate {
                    thumb(up: true, rate)
                    thumb(up: false, rate)
                }
            }
            .font(.footnote)
            .foregroundStyle(Color.secondary)
            .buttonStyle(.borderless)
        }
        .frame(maxWidth: .infinity, alignment: .leading)
        .sensoryFeedback(.success, trigger: copied)
        .contextMenu {
            Button("Copy", systemImage: "doc.on.doc") { UIPasteboard.general.string = text }
            ShareLink(item: text) { Label("Share", systemImage: "square.and.arrow.up") }
            Button("Report reply", systemImage: "flag") { report(String(text.prefix(4000))) }
        }
    }

    /// A thumb: pressing it sets it, and pressing the one that's set takes it off.
    private func thumb(up: Bool, _ rate: @escaping (Bool?) -> Void) -> some View {
        let on = rating == (up ? "up" : "down")
        let symbol = up ? "hand.thumbsup" : "hand.thumbsdown"
        return Button {
            rate(on ? nil : up)
        } label: {
            Image(systemName: on ? symbol + ".fill" : symbol)
        }
        .foregroundStyle(on ? Color.urAccent : Color.secondary)
        .accessibilityLabel(up ? Text("Good reply") : Text("Bad reply"))
        .accessibilityAddTraits(on ? .isSelected : [])
    }
}

/// The reply as it streams in, drawn as Markdown like a finished one so nothing jumps when it
/// completes, with the model's thinking folded away.
private struct AssistantLiveReply: View {
    var controller: AssistantController
    var scroll: AssistantScroll
    var proxy: ScrollViewProxy
    @State private var thinkingShown = false

    var body: some View {
        let blocks = controller.liveBlocks
        let thinking = controller.thinking
        VStack(alignment: .leading, spacing: 8) {
            if !thinking.isEmpty {
                DisclosureGroup(isExpanded: $thinkingShown) {
                    Text(thinking).font(.footnote).foregroundStyle(Color.secondary).frame(maxWidth: .infinity, alignment: .leading)
                } label: {
                    Label("Thinking", systemImage: "ellipsis.bubble").font(.footnote.weight(.semibold)).foregroundStyle(Color.secondary)
                }
            }
            if !blocks.isEmpty {
                // Spaced as a finished reply's blocks are.
                VStack(alignment: .leading, spacing: 10) {
                    ForEach(Array(blocks.indices), id: \.self) { index in
                        AssistantMarkdownBlockView(block: blocks[index])
                    }
                }
                .frame(maxWidth: .infinity, alignment: .leading)
            }
        }
        .padding(.vertical, blocks.isEmpty && thinking.isEmpty ? 0 : 7)
        .onChange(of: blocks) { _, _ in
            // New text is followed only while the end of the conversation is on screen.
            if scroll.following { proxy.scrollTo("bottom", anchor: .bottom) }
        }
    }
}

/// An attachment or the selected transactions above the composer, with a remove button.
private struct AssistantChip: View {
    var text: String
    var symbol: String
    var removeLabel: String
    var onTap: (() -> Void)?
    var remove: () -> Void

    var body: some View {
        HStack(spacing: 6) {
            Button { onTap?() } label: {
                Label(text, systemImage: symbol).font(.footnote).lineLimit(1)
            }
            .buttonStyle(.plain)
            .disabled(onTap == nil)
            Button(action: remove) {
                Image(systemName: "xmark.circle.fill").foregroundStyle(Color.secondary)
            }
            .buttonStyle(.plain)
            .accessibilityLabel(removeLabel)
        }
        .padding(.leading, 10)
        .padding(.trailing, 6)
        .padding(.vertical, 6)
        .background(Color.cardBackground, in: Capsule())
    }
}

/// What gets sent, and to whom, shown before the first message to each service. Continue has the
/// service remember it was read, then sends what waited on it; Not now sends nothing and leaves
/// the message as typed. The memo switch is here so it can be turned off before anything goes.
private struct AssistantSendNotice: View {
    /// Called once the service has it that the notice was read.
    var proceed: () -> Void
    @Environment(\.dismiss) private var dismiss
    private let controller = AssistantController.shared

    var body: some View {
        let company = AssistantNotice.recipient(controller.provider)
        NavigationStack {
            Form {
                Section {
                    Text("Your messages, what's on screen, the files you attach, and the budget details the assistant looks up to answer you go to \(company). Its terms and privacy policy govern how it processes and retains them.")
                        .font(.subheadline)
                    Text("Your budget file, conversation history and sign-in are kept on this device. Nothing in the budget changes until you apply a change yourself.")
                        .font(.subheadline)
                }
                Section {
                    AssistantMemosToggle()
                }
                if let error = controller.error { FormErrorView(message: error) }
            }
            .navigationTitle("What gets sent")
            .inlineTitle()
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button("Not now") {
                        controller.dismissError()
                        dismiss()
                    }
                }
                ToolbarItem(placement: .confirmationAction) {
                    Button("Continue") { controller.settings(["told": true], done: { proceed() }) }
                        .fontWeight(.semibold)
                        .disabled(controller.busy.contains("settings"))
                }
            }
        }
        .presentationDetents([.medium, .large])
    }
}

extension String {
    fileprivate var nilIfEmpty: String? { isEmpty ? nil : self }
}
