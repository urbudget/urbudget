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

#if DEBUG
/// A debug build opens a saved conversation at launch, for screenshots:
/// `-open assistant -conversation <id>`. Release builds have no such thing.
enum AssistantLaunch {
    nonisolated(unsafe) static var conversation: String? = {
        let arguments = ProcessInfo.processInfo.arguments
        guard let index = arguments.firstIndex(of: "-conversation"), arguments.indices.contains(index + 1) else { return nil }
        return arguments[index + 1]
    }()
}
#endif

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
    /// How tall the content was when it last moved.
    @ObservationIgnored private var height: CGFloat = 0

    /// The end of the conversation is on screen now. Not observed: nothing is drawn from it.
    @ObservationIgnored private(set) var atEnd = true

    /// The content moved under the finger, or was laid out again. True when the list should be
    /// brought to its end again (`AssistantFollow.pins`): it grew past the screen's end while
    /// that end is followed.
    @discardableResult
    func moved(_ extent: AssistantScrollExtent) -> Bool {
        let was = height
        height = extent.bottom - extent.top
        // A finger moves the content and leaves its height alone.
        let relaid = abs(height - was) > 0.5
        let next = AssistantFollow.next(following, top: extent.top, restingTop: restingTop, bottom: extent.bottom, viewport: viewport, relaid: relaid)
        restingTop = AssistantFollow.resting(restingTop, top: extent.top, bottom: extent.bottom, viewport: viewport)
        atEnd = extent.bottom <= viewport + AssistantFollow.slack
        if next != following { following = next }
        return AssistantFollow.pins(following: next, height: height, was: was, bottom: extent.bottom, viewport: viewport)
    }

    /// The scroll view's height changed: the keyboard, or a longer message in the field.
    func resized(_ height: CGFloat) { viewport = height }

    /// Another conversation, a message sent, or Jump to latest: at the end, following again.
    func follow() {
        restingTop = .greatestFiniteMagnitude
        height = 0
        if !following { following = true }
    }

    /// Something above the end is being read where it stands (a finished card was opened):
    /// new text isn't followed until the end is back on screen.
    func hold() {
        if following { following = false }
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
    /// Going to the conversation's end, again until it's there (`toEnd`).
    @State private var settling: Task<Void, Never>?
    @FocusState private var composing: Bool
    @ScaledMetric(relativeTo: .body) private var iconSize: CGFloat = 36

    private let request: AssistantRequest?
    /// Open beside the app on a wide window, not as a sheet over it: Done closes the panel, and
    /// what a card shows in the budget appears beside the conversation, which stays open.
    private let beside: Bool
    /// Beside the app: the page the app is on now, which the next message is said to be about.
    private let place: String?

    init(request: AssistantRequest? = nil, beside: Bool = false, place: String? = nil) {
        self.request = request
        self.beside = beside
        self.place = place
    }

    /// Closes the assistant: its panel beside the app, or its sheet.
    private func close() {
        if beside { AppActions.shared.panel = nil } else { dismiss() }
    }

    private var sheetShown: Bool {
        historyShown || contextShown || afterNotice != nil || report != nil || importReview != nil || replyReport != nil || share != nil || preview != nil || changing != nil
    }

    /// The open budget's currency, for the amounts on cards and in an exported conversation.
    private var currency: String { store.budget?.text("currency") ?? "" }

    /// What goes with the next message: where this sheet was opened from, the month the app is
    /// on, the selected transactions and the file being imported, less what was taken off.
    private var with: AssistantWith {
        // Beside the app the page follows the app; what the request named of its own page
        // (an account, a category) goes with it only while the app is still there.
        let asked = request?.page ?? "assistant"
        let page = place ?? asked
        let there = page == asked
        return AssistantWith(page: page, month: store.month, account: there ? request?.account ?? "" : "", category: there ? request?.category ?? "" : "",
                             selected: controller.selected, importing: request?.importing, placeOff: placeOff, importOff: importOff)
    }

    var body: some View {
        NavigationStack {
            conversation
                .background(Color.groupedBackground)
                .safeAreaInset(edge: .top, spacing: 0) {
                    VStack(spacing: 0) {
                        titleRow
                        noticeBar
                        quietBar
                    }
                }
                .safeAreaInset(edge: .bottom, spacing: 0) { composer }
                // In a sheet it doesn't take the window's "no scroll bars".
                .scrollIndicators(.hidden)
                // Beside the app on a fold's far side, it has that side to its edge.
                .awayFromSideBar()
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
            #if DEBUG
            if let id = AssistantLaunch.conversation {
                AssistantLaunch.conversation = nil
                controller.openThread(id)
            }
            #endif
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
        // The budget changed under the open conversation (sync, another screen, its own Undo or
        // Redo): its waiting cards are brought up to date, and an applied one is shown where it stands.
        .onChange(of: store.revision) { _, _ in controller.budgetChanged() }
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
                // Made on the person's choices as they stand now, not as they were when the editor opened.
                let id = item.value.proposal.text("id")
                controller.choose(id, ProposalChoices.change(controller.choicesNow(id), key: item.value.row.text("key"), change: change))
            }
        }
        .sheet(item: $share) { AssistantShareSheet(items: [$0.value]) { share = nil }.sheetDetents([.medium, .large]) }
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
        let rows = AssistantRows.of(controller.entries)
        // Found once for the list: which reply is the latest, and where the turn being answered begins.
        let marks = AssistantReplyTools.marks(controller.entries.map(\.kind))
        return ScrollViewReader { proxy in
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
                    // Lookups in a row are one row; a kind this version doesn't know takes no room at all.
                    ForEach(rows) { row in
                        rowView(row, marks: marks, proxy: proxy).padding(.vertical, 7).id(Self.rowID(row.id))
                    }
                    AssistantLiveReply(controller: controller, scroll: scroll, proxy: proxy)
                    if controller.running || controller.busy.contains("send") {
                        AssistantWorkingRow(controller: controller)
                    } else if controller.entries.last?.kind == "user" {
                        Button("Continue interrupted reply", systemImage: "arrow.clockwise") { retry() }
                            .font(.subheadline)
                            .disabled(controller.busy.contains("send"))
                            .padding(.vertical, 8)
                    } else if !controller.next.isEmpty {
                        nextSteps
                    }
                    Color.clear.frame(height: 1).id("bottom")
                }
                .padding(.horizontal, 16)
                .padding(.vertical, 8)
                .onGeometryChange(for: AssistantScrollExtent.self) { geometry in
                    let frame = geometry.frame(in: .named("assistantScroll"))
                    return AssistantScrollExtent(top: frame.minY, bottom: frame.maxY)
                } action: { extent in
                    // A lazy stack measures its rows as they come on screen: a conversation
                    // opened at its end would otherwise rest wherever the first guess left it.
                    if scroll.moved(extent) { toEnd(proxy) }
                }
            }
            // A new conversation keeps this scroll view when its first message gives it an id.
            .id(controller.conversation)
            .coordinateSpace(.named("assistantScroll"))
            // A conversation stands on its latest line; before there is one, what the assistant is
            // for starts at the top of the page, as on Android, where it's all in view.
            .defaultScrollAnchor(controller.entries.isEmpty && !controller.running && controller.threadId.isEmpty ? .top : .bottom)
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
                    toEnd(proxy)
                }
            }
            .onChange(of: controller.conversation) { _, _ in scroll.follow() }
            .onChange(of: controller.entries.count) { _, count in
                if scroll.following || count < 4 { toEnd(proxy) }
            }
            .onChange(of: controller.running) { _, _ in
                if scroll.following { toEnd(proxy) }
            }
            .onChange(of: sent) { _, _ in
                scroll.follow()
                toEnd(proxy)
            }
        }
    }

    /// Brings the list to its very end, straight there. A lazy stack only guesses at the rows
    /// it hasn't drawn, so one go can land short of the end, and an animated one sets off for
    /// where the end was guessed to be: it's gone to again until it's on screen, unless the
    /// person scrolls away meanwhile.
    private func toEnd(_ proxy: ScrollViewProxy) {
        proxy.scrollTo("bottom", anchor: .bottom)
        settling?.cancel()
        settling = Task { @MainActor in
            for _ in 0..<20 {
                try? await Task.sleep(for: .milliseconds(50))
                guard !Task.isCancelled, scroll.following, !scroll.atEnd else { return }
                proxy.scrollTo("bottom", anchor: .bottom)
            }
        }
    }

    /// What's waiting in the budget that the assistant can help with, under the last reply while
    /// nothing is running: each a small button in the service's words that sends its request.
    private var nextSteps: some View {
        // Small ones in a row that wraps: stacked, each stretched down the screen under the reply.
        AssistantFlow(spacing: 8, lineSpacing: 8) {
            ForEach(Array(controller.next.enumerated()), id: \.offset) { _, step in
                Button(step.text("text")) { say(step.text("prompt")) }
                    .buttonStyle(.bordered)
                    .buttonBorderShape(.capsule)
                    .controlSize(.small)
                    .disabled(!canSay)
            }
        }
        .frame(maxWidth: .infinity, alignment: .leading)
        .padding(.vertical, 7)
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
                    .buttonBorderShape(.capsule)
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

    /// A row's name in the scroll view, for going to it.
    private static func rowID(_ id: Int) -> String { "entry-\(id)" }

    @ViewBuilder
    private func rowView(_ row: AssistantRow, marks: (user: Int, assistant: Int), proxy: ScrollViewProxy) -> some View {
        if row.kind == "step" {
            AssistantSteps(steps: row.entries)
        } else if let entry = row.entries.first {
            entryView(entry, marks: marks, proxy: proxy)
        }
    }

    @ViewBuilder
    private func entryView(_ entry: AssistantEntry, marks: (user: Int, assistant: Int), proxy: ScrollViewProxy) -> some View {
        switch entry.kind {
        case "proposal":
            let proposal = entry.json["proposal"]
            if proposal.isObject {
                let id = proposal.text("id")
                // Only its own action makes a card wait, and the button that asked spins. Undo
                // shows on the card the conversation's view names as the one that can be undone.
                let working = controller.cardWork[id]
                AssistantProposalCard(proposal: proposal, currency: currency, busy: working != nil, working: working,
                                      choices: controller.choices[id], canUndo: controller.undoable == id, canSend: canSay,
                                      categoryName: { planningCategoryName(store.budget, $0 ?? Unassigned) ?? "" },
                                      actions: actions(for: proposal), cards: controller.cards) { opening in
                    // A card opened is being read where it stands. With the end on screen the list
                    // would hold to it as the card grew, and take the card's title out of view.
                    guard opening, scroll.following else { return }
                    scroll.hold()
                    let row = Self.rowID(entry.id)
                    Task { @MainActor in
                        withAnimation(.easeOut(duration: 0.2)) { proxy.scrollTo(row, anchor: .top) }
                    }
                }
            }
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
            let text = entry.json.text("text")
            AssistantReply(text: text, blocks: entry.blocks, whole: controller.wasWritten(text),
                           tools: AssistantReplyTools.of(entry.id, marks: marks, running: controller.running),
                           rating: entry.json.text("rating"), rate: rating(entry.json)) { excerpt in
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
            restore: { controller.restore(id) },
            refine: {
                // Added under whatever is being written, never over it.
                controller.addToDraft(String(localized: "Revise the proposal ‘\(proposal.text("title"))’: "))
                composing = true
            },
            askAgain: { say(AssistantCard.askAgain(proposal)) },
            show: { show(proposal["card"]["highlight"]) },
            answer: { say($0) },
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
            // The import applies the card as the service has it once its waiting choices are sent.
            applyFix: request?.applyFix.map { fix -> () -> Void in { controller.whenSettled(id) { thread, settled in fix(thread, settled) } } },
            // Back to the assistant's version goes at once; a tick waits a moment for the next.
            revise: { revision in controller.choose(id, revision, now: revision == ProposalChoices.original) },
            change: { changing = AssistantSheetItem(value: AssistantRowChange(proposal: proposal, row: $0)) })
    }

    /// A message can be sent for the person now: a question's answer, a next step, or a card
    /// asked for again.
    private var canSay: Bool { controller.ready && !controller.running && !controller.busy.contains("send") }

    /// Sends `text` as the person's message, as Send does the field's: after the notice of what
    /// gets sent when it's due, and with what's on "With this message". The message being
    /// written stays in the field.
    private func say(_ text: String) {
        guard canSay, !text.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty else { return }
        whenTold {
            sent += 1
            controller.send(text, screen: with.screen) {
                placeOff = false
                importOff = false
            }
        }
    }

    /// Goes to what a card names, once the assistant has closed: its transactions in the
    /// Transactions tab, or else the Budget tab on its month.
    private func show(_ highlight: JSON) {
        let destination = AppActions.Destination(month: highlight.text("month"), transactions: highlight["transactions"].array.map(\.string))
        if !beside { dismiss() }
        AppActions.shared.destination = destination
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
            Button("Done") { close() }
        }
        ToolbarItem(placement: .principal) {
            HStack(spacing: 6) {
                Text("Assistant").font(.headline)
                if !controller.busy.subtracting(["send", "models", "status"]).isEmpty {
                    ProgressView().controlSize(.mini).accessibilityLabel("Loading")
                }
            }
            .accessibilityElement(children: .combine)
        }
        ToolbarItemGroup(placement: .trailingBar) {
            Button { historyShown = true } label: { Image(systemName: "clock.arrow.circlepath") }
                .accessibilityLabel("Conversation history")
            // What's typed comes along; a reply still running in the conversation left carries on there.
            Button { controller.startNew() } label: { Image(systemName: "square.and.pencil") }
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

    /// Under the bar's name and its buttons, the open conversation's title with the screen's
    /// width to itself: between the bar's buttons a phone had room for half of it. A tap
    /// renames it. With no conversation yet, what the assistant is for.
    private var titleRow: some View {
        let title = controller.thread?.text("title") ?? ""
        return Button {
            newTitle = title
            renaming = true
        } label: {
            HStack(spacing: 4) {
                Text(title.isEmpty ? String(localized: "Your budget, with a second pair of eyes") : title).lineLimit(1)
                if !title.isEmpty { Image(systemName: "pencil").font(.caption2).accessibilityHidden(true) }
            }
            .font(.footnote)
            .foregroundStyle(Color.secondary)
            .frame(maxWidth: .infinity)
            .padding(.horizontal, 16)
            .padding(.top, 2)
            .padding(.bottom, 6)
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .disabled(title.isEmpty || controller.threadId.isEmpty)
        .accessibilityHint(title.isEmpty ? Text(verbatim: "") : Text("Rename conversation"))
        .background(Color.groupedBackground)
    }

    /// Something done unasked failed (reading the conversation again, a refresh on coming
    /// back): a line here, not an alert.
    @ViewBuilder
    private var quietBar: some View {
        if let quiet = controller.quiet.line {
            HStack(alignment: .firstTextBaseline, spacing: 8) {
                Text(quiet).font(.footnote).foregroundStyle(Color.urWarning).frame(maxWidth: .infinity, alignment: .leading)
                Button { controller.dismissQuiet() } label: {
                    Image(systemName: "xmark").font(.footnote.weight(.semibold)).foregroundStyle(Color.secondary)
                        .frame(minWidth: 32, minHeight: 32).contentShape(Rectangle())
                }
                .buttonStyle(.plain)
                .accessibilityLabel("Dismiss")
            }
            .padding(.leading, 16)
            .padding(.trailing, 8)
            .background(Color.groupedBackground)
        }
    }

    @ViewBuilder
    private var noticeBar: some View {
        if let notice = controller.notice {
            // An import's notice offers Undo while the import is still the budget's last change.
            let undo = controller.undoNotice?.text == notice && controller.undoNotice?.revision == store.revision
            Notice(text: notice, tone: "info", icon: "info.circle.fill", action: undo ? String(localized: "Undo") : String(localized: "Dismiss")) {
                if undo { store.undo() }
                controller.dismissNotice()
            }
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

    /// Above the field in a conversation that can't be carried on as things stand: the
    /// assistant couldn't be loaded, a sign-in is under way or needed, there's no key, or the
    /// conversation has grown long. Each has the one action that helps, not just a greyed Send.
    @ViewBuilder
    private var composerBanner: some View {
        let started = !controller.entries.isEmpty || !(controller.thread?.text("continuedFrom") ?? "").isEmpty
        switch AssistantComposerBanner.of(started: started, status: controller.status, statusFailed: controller.statusFailed, signingIn: controller.signingIn,
                                          long: controller.thread?.flag("long") == true, running: controller.running) {
        case .notLoaded:
            AssistantBanner(text: String(localized: "The assistant couldn't be loaded."), action: String(localized: "Try again"),
                            busy: controller.busy.contains("refresh")) { controller.refresh() }
        case .signingIn:
            AssistantBanner(text: String(localized: "Waiting for ChatGPT…"), action: String(localized: "Cancel sign-in")) { controller.cancelSignIn() }
        case .signIn:
            AssistantBanner(text: String(localized: "Set up the assistant to send messages"), action: String(localized: "Continue with ChatGPT")) {
                controller.signIn()
            }
        case .setUp:
            AssistantBanner(text: String(localized: "Set up the assistant to send messages"), action: String(localized: "Assistant settings")) {
                settingsShown = true
            }
        case .long:
            AssistantBanner(text: String(localized: "This conversation is getting long, so replies are slower and use more of your plan or key."),
                            action: String(localized: "Continue in a new conversation"), busy: controller.busy.contains("continue")) {
                controller.continueInNew()
            }
        case nil:
            EmptyView()
        }
    }

    /// What goes with the next message, each part with its own ×, and the sentence for a
    /// selection that was cut to its first 200.
    @ViewBuilder
    private var withLine: some View {
        // Where the person is, alone, is listed only where a message can go.
        let parts = AssistantWith.listed(with.parts(budget: store.budget), canSend: controller.ready || controller.status == nil)
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

/// "Working…", or what the assistant is doing now while no text is arriving, and the latest
/// line of its thinking. A view of its own, so a new activity redraws this row and not the
/// conversation.
private struct AssistantWorkingRow: View {
    var controller: AssistantController

    var body: some View {
        let activity = controller.activity
        // What it's doing reads as under way: "Looking at October…".
        let doing = activity.isEmpty || activity.hasSuffix("…") ? activity : activity + "…"
        let thinking = controller.thinking
        VStack(alignment: .leading, spacing: 4) {
            HStack(spacing: 8) {
                ProgressView().controlSize(.small)
                Text(doing.isEmpty ? String(localized: "Working…") : doing).font(.footnote).foregroundStyle(Color.secondary)
            }
            // The latest line of its thinking: one line, however long it thinks.
            if !thinking.isEmpty {
                Text("Thinking: \(thinking)").font(.caption).foregroundStyle(Color.secondary).lineLimit(1).truncationMode(.tail)
            }
        }
        .padding(.vertical, 8)
        .accessibilityElement(children: .combine)
        .accessibilityLabel(doing.isEmpty ? String(localized: "The assistant is working") : doing)
    }
}

/// Jump to latest, over the end of the conversation once the person has scrolled up from it. A
/// small round button, as a phone's messages have: it covers a word or two of what's being
/// read, where a line of words across the middle covered a card's row or its buttons.
private struct AssistantJumpButton: View {
    var scroll: AssistantScroll
    var action: () -> Void

    var body: some View {
        if !scroll.following {
            Button(action: action) {
                Image(systemName: "arrow.down")
                    .font(.body.weight(.semibold))
                    .foregroundStyle(Color.urAccent)
                    .padding(10)
                    .background(.regularMaterial, in: Circle())
                    .overlay(Circle().strokeBorder(Color.secondary.opacity(0.25)))
                    .shadow(color: .black.opacity(0.12), radius: 3, y: 1)
            }
            .buttonStyle(.plain)
            .accessibilityLabel("Jump to latest")
            .padding(.bottom, 2)
        }
    }
}

/// Lookups in a row: the latest and any that failed, with the rest a tap away. What was looked
/// up is a note beside the conversation: smaller and quieter than a reply's words or a card's title.
private struct AssistantSteps: View {
    var steps: [AssistantEntry]
    @State private var open = false

    var body: some View {
        let shown = AssistantRows.shown(steps, open: open)
        let more = steps.count - shown.count
        VStack(alignment: .leading, spacing: 2) {
            ForEach(Array(shown.enumerated()), id: \.element.id) { index, step in
                // The latest line says how many more there are, and is the button that shows
                // them: tall enough to tap, though its words are small.
                if more > 0 && index == shown.count - 1 {
                    Button { withAnimation(.easeOut(duration: 0.2)) { open = true } } label: {
                        line(step, more: more).frame(minHeight: 44).contentShape(Rectangle())
                    }
                    .buttonStyle(.plain)
                } else {
                    line(step, more: 0)
                }
            }
            if open && steps.count > AssistantRows.shown(steps, open: false).count {
                Button("Show fewer") { withAnimation(.easeOut(duration: 0.2)) { open = false } }
                    .font(.caption.weight(.semibold))
                    .buttonStyle(.borderless)
                    .padding(.leading, 20)
                    .frame(minHeight: 44)
            }
        }
        .frame(maxWidth: .infinity, alignment: .leading)
    }

    private func line(_ step: AssistantEntry, more: Int) -> some View {
        let failed = step.json.flag("failed")
        let text = step.json.text("text")
        return HStack(alignment: .firstTextBaseline, spacing: 6) {
            Image(systemName: failed ? "exclamationmark.circle" : "checkmark")
                .font(.caption2.weight(.semibold))
                .frame(width: 14)
                .accessibilityHidden(true)
            Text(failed ? String(localized: "Couldn’t complete: \(text)") : text)
                .fixedSize(horizontal: false, vertical: true)
            if more > 0 {
                Text("\(more) more steps").foregroundStyle(Color.urAccent).lineLimit(1).layoutPriority(1)
            }
        }
        .font(.caption)
        .foregroundStyle(failed ? Color.urWarning : Color.secondary)
        .accessibilityElement(children: .combine)
    }
}

/// Wraps its children onto as many lines as they need: the buttons for what's waiting.
private struct AssistantFlow: Layout {
    var spacing: CGFloat = 8
    var lineSpacing: CGFloat = 8

    private func sizes(_ subviews: Subviews, width: CGFloat) -> [CGSize] {
        subviews.map { view in
            let ideal = view.sizeThatFits(.unspecified)
            return ideal.width > width ? view.sizeThatFits(ProposedViewSize(width: width, height: nil)) : ideal
        }
    }

    func sizeThatFits(proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) -> CGSize {
        let width = proposal.width ?? .infinity
        var x: CGFloat = 0, y: CGFloat = 0, line: CGFloat = 0, widest: CGFloat = 0
        for size in sizes(subviews, width: width) {
            if x > 0 && x + size.width > width {
                y += line + lineSpacing
                x = 0
                line = 0
            }
            x += size.width + spacing
            line = max(line, size.height)
            widest = max(widest, x - spacing)
        }
        return CGSize(width: widest, height: y + line)
    }

    func placeSubviews(in bounds: CGRect, proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) {
        var x: CGFloat = 0, y: CGFloat = 0, line: CGFloat = 0
        for (view, size) in zip(subviews, sizes(subviews, width: bounds.width)) {
            if x > 0 && x + size.width > bounds.width {
                y += line + lineSpacing
                x = 0
                line = 0
            }
            view.place(at: CGPoint(x: bounds.minX + x, y: bounds.minY + y), proposal: ProposedViewSize(size))
            x += size.width + spacing
            line = max(line, size.height)
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

/// The assistant's reply in Markdown, with copying, sharing, reporting and rating it to hand:
/// out under the latest reply, behind one quiet button under an earlier one, and not there yet
/// while its turn is still answered (`AssistantReplyTools`).
private struct AssistantReply: View {
    var text: String
    var blocks: [AssistantMarkdownBlock]
    /// It was written live while the assistant was open: it's shown whole, as it was then.
    var whole = false
    var tools = AssistantReplyTools.shown
    /// The person's thumb on this reply: "up", "down", or nothing.
    var rating: String
    /// Sets the thumb (true is up), or takes it off (nil). Nil for a reply that can't be rated.
    var rate: ((Bool?) -> Void)?
    var report: (String) -> Void
    @State private var copied = 0
    /// An earlier reply's buttons were asked for.
    @State private var unfolded = false

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            AssistantMarkdownView(blocks: blocks, whole: whole)
            if tools == .folded && !unfolded {
                Button { unfolded = true } label: {
                    Image(systemName: "ellipsis")
                        .font(.footnote.weight(.semibold))
                        .foregroundStyle(Color.secondary)
                        .frame(minWidth: 32, minHeight: 24, alignment: .leading)
                        .contentShape(Rectangle())
                }
                .buttonStyle(.plain)
                .accessibilityLabel("Copy, report or rate this reply")
            } else if tools != .waiting {
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
/// completes. Only this redraws as its words arrive.
private struct AssistantLiveReply: View {
    var controller: AssistantController
    var scroll: AssistantScroll
    var proxy: ScrollViewProxy

    var body: some View {
        let blocks = controller.liveBlocks
        VStack(alignment: .leading, spacing: 8) {
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
        .padding(.vertical, blocks.isEmpty ? 0 : 7)
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
            CompactForm {
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
        .sheetDetents([.medium, .large])
    }
}

