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
/// put in the composer for the person to send, as the desktop's ask() does.
struct AssistantRequest: Identifiable {
    let id = UUID()
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

/// The assistant: a conversation about the budget whose changes arrive as proposals to review,
/// apply and undo. Presented as a large sheet with its own navigation stack.
struct AssistantView: View {
    @Environment(BudgetStore.self) private var store
    @Environment(\.dismiss) private var dismiss
    @Environment(\.scenePhase) private var phase
    private let controller = AssistantController.shared

    @State private var message = ""
    @State private var selected: [String] = []
    @State private var settingsShown = false
    @State private var historyShown = false
    @State private var contextShown = false
    @State private var choosingFiles = false
    @State private var choosingPhotos = false
    @State private var photoItems: [PhotosPickerItem] = []
    @State private var report: AssistantSheetItem<JSON>?
    @State private var importReview: AssistantSheetItem<AssistantImportRequest>?
    @State private var replyReport: AssistantSheetItem<String>?
    @State private var share: AssistantSheetItem<URL>?
    @State private var changing: AssistantSheetItem<AssistantRowChange>?
    @State private var preview: URL?
    @State private var localError: String?
    @State private var sent = 0
    @State private var following = true
    @FocusState private var composing: Bool
    @ScaledMetric(relativeTo: .body) private var iconSize: CGFloat = 36

    private let request: AssistantRequest?

    init(request: AssistantRequest? = nil) { self.request = request }

    private var sheetShown: Bool {
        historyShown || contextShown || report != nil || importReview != nil || replyReport != nil || share != nil || preview != nil || changing != nil
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
        }
        .task {
            controller.connect(store)
            controller.refresh()
            if let request {
                if !request.selected.isEmpty { selected = request.selected }
                if !request.text.isEmpty && message.isEmpty { message = request.text }
                if !request.files.isEmpty { controller.attach(files: request.files) }
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
        .onReceive(NotificationCenter.default.publisher(for: .urbudgetHostEvent)) { notification in
            if let event = notification.hostEvent { controller.receive(event.name, event.payload) }
        }
        .onChange(of: phase) { _, phase in if phase == .active { controller.refresh() } }
        .onChange(of: store.identity) { _, _ in
            controller.connect(store)
            message = ""
            selected = []
            controller.refresh()
        }
        .sensoryFeedback(.impact(weight: .light), trigger: sent)
        .sensoryFeedback(.success, trigger: controller.successes)
        .fileImporter(isPresented: $choosingFiles, allowedContentTypes: AssistantAttachments.contentTypes,
                      allowsMultipleSelection: true) { result in
            switch result {
            case .success(let urls):
                let room = max(0, AssistantAttachments.limit - controller.attachments.count)
                if urls.count > room { localError = "Attach up to five files per message." }
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
        .sheet(isPresented: $contextShown) { AssistantContextPicker(selected: $selected) }
        .sheet(item: $report) { AssistantReportView(report: $0.value) }
        .sheet(item: $importReview) { item in
            AssistantImportReview(request: item.value) {
                controller.imported(thread: item.value.threadId, proposal: item.value.proposalId)
            }
        }
        .sheet(item: $replyReport) { AssistantReplyReportForm(excerpt: $0.value) }
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

    private var conversation: some View {
        ScrollViewReader { proxy in
            ScrollView {
                LazyVStack(alignment: .leading, spacing: 0) {
                    if controller.entries.isEmpty && !controller.running && controller.threadId.isEmpty {
                        emptyState.padding(.vertical, 12)
                    }
                    ForEach(controller.entries) { entry in
                        entryView(entry).padding(.vertical, 7)
                    }
                    AssistantLiveReply(controller: controller, proxy: proxy, following: following)
                    if controller.running || controller.busy.contains("send") {
                        HStack(spacing: 8) {
                            ProgressView().controlSize(.small)
                            Text("Working…").font(.footnote).foregroundStyle(Color.secondary)
                        }
                        .padding(.vertical, 8)
                        .accessibilityElement(children: .combine)
                        .accessibilityLabel("The assistant is working")
                    } else if controller.entries.last?.kind == "user" {
                        Button("Continue interrupted reply", systemImage: "arrow.clockwise") { controller.retry(importing: request?.importing) }
                            .font(.subheadline)
                            .disabled(controller.busy.contains("send"))
                            .padding(.vertical, 8)
                    }
                    Color.clear.frame(height: 1).id("bottom")
                        .onAppear { following = true }
                        .onDisappear { following = false }
                }
                .padding(.horizontal, 16)
                .padding(.vertical, 8)
            }
            .id(controller.threadId)
            .defaultScrollAnchor(.bottom)
            .scrollDismissesKeyboard(.interactively)
            .onChange(of: controller.entries.count) { _, count in
                if following || count < 4 { withAnimation(.easeOut(duration: 0.25)) { proxy.scrollTo("bottom", anchor: .bottom) } }
            }
            .onChange(of: controller.running) { _, _ in
                if following { withAnimation(.easeOut(duration: 0.25)) { proxy.scrollTo("bottom", anchor: .bottom) } }
            }
            .onChange(of: sent) { _, _ in withAnimation { proxy.scrollTo("bottom", anchor: .bottom) } }
        }
    }

    private var emptyState: some View {
        VStack(alignment: .leading, spacing: 12) {
            Image(systemName: "sparkles").font(.largeTitle).foregroundStyle(Color.urBrand).accessibilityHidden(true)
            Text("Ask about your budget. Review every change.").font(.title2.bold())
            Text("Look up spending, cover overspending, categorize selected transactions, read statements, or create a report. Proposed changes are checked again before you apply them.")
                .font(.subheadline).foregroundStyle(Color.secondary)
            if controller.status == nil {
                ProgressView().padding(.top, 8)
            } else if !controller.ready {
                Button("Set up assistant") { settingsShown = true }
                    .buttonStyle(.borderedProminent)
                    .padding(.top, 4)
            } else {
                VStack(spacing: 8) {
                    ForEach(["Where is my money going this month?", "Help me cover overspending", "Make a report of this month's budget"], id: \.self) { prompt in
                        Button {
                            message = prompt
                            composing = true
                        } label: {
                            HStack {
                                Text(prompt).multilineTextAlignment(.leading)
                                Spacer(minLength: 8)
                                Image(systemName: "arrow.up.left").font(.footnote).foregroundStyle(Color.secondary)
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

    @ViewBuilder
    private func entryView(_ entry: AssistantEntry) -> some View {
        switch entry.kind {
        case "proposal":
            let proposal = entry.json["proposal"]
            if proposal.isObject {
                AssistantProposalCard(proposal: proposal, currency: store.currency,
                                      busy: controller.busy.contains("proposal"),
                                      canUndo: controller.undoProposal == proposal.text("id"),
                                      actions: actions(for: proposal))
            }
        case "step":
            let failed = entry.json.flag("failed")
            Label {
                Text(failed ? "Couldn’t complete: \(entry.json.text("text"))" : entry.json.text("text"))
            } icon: {
                Image(systemName: failed ? "exclamationmark.circle" : "checkmark.circle")
            }
            .font(.footnote)
            .foregroundStyle(failed ? Color.urNegative : .secondary)
        case "stopped":
            Label("Reply stopped", systemImage: "stop.circle").font(.footnote).foregroundStyle(Color.secondary)
        case "user":
            AssistantUserBubble(entry: entry.json)
        case "error":
            VStack(alignment: .leading, spacing: 8) {
                Label("Couldn’t finish", systemImage: "exclamationmark.triangle.fill").font(.subheadline.weight(.semibold)).foregroundStyle(Color.urNegative)
                Text(entry.json.text("text")).font(.subheadline)
                Button("Try again") { controller.retry(importing: request?.importing) }
                    .font(.subheadline.weight(.semibold))
                    .disabled(controller.running || controller.busy.contains("send"))
            }
            .padding(14)
            .frame(maxWidth: .infinity, alignment: .leading)
            .background(Color.urNegative.opacity(0.1), in: RoundedRectangle(cornerRadius: 14, style: .continuous))
        default:
            AssistantReply(text: entry.json.text("text"), blocks: entry.blocks) { excerpt in
                replyReport = AssistantSheetItem(value: excerpt)
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
                message = "Revise the proposal ‘\(proposal.text("title"))’: "
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
                controller.spreadsheet(id) { path in
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
        let currency = store.currency
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
                Text(controller.thread?.text("title").nilIfEmpty ?? "Your budget, with a second pair of eyes")
                    .font(.caption2).foregroundStyle(Color.secondary).lineLimit(1)
            }
            .accessibilityElement(children: .combine)
        }
        ToolbarItemGroup(placement: .trailingBar) {
            Button { historyShown = true } label: { Image(systemName: "clock.arrow.circlepath") }
                .accessibilityLabel("Conversation history")
            Button {
                controller.newChat()
                message = ""
                selected = []
            } label: { Image(systemName: "square.and.pencil") }
                .accessibilityLabel("New conversation")
            Menu {
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
            Notice(text: notice, tone: "info", icon: "info.circle.fill", action: "Dismiss") { controller.dismissNotice() }
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
            && (!message.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty || !controller.attachments.isEmpty)
    }

    private func send() {
        guard canSend else { return }
        let text = message
        sent += 1
        controller.send(text, selected: selected, importing: request?.importing) {
            if message == text { message = "" }
            selected = []
        }
    }

    private var composer: some View {
        VStack(alignment: .leading, spacing: 8) {
            if !controller.attachments.isEmpty || !selected.isEmpty || controller.busy.contains("attach") {
                ScrollView(.horizontal, showsIndicators: false) {
                    HStack(spacing: 8) {
                        ForEach(Array(controller.attachments.enumerated()), id: \.offset) { _, file in
                            AssistantChip(text: file.text("name"), symbol: AssistantAttachments.symbol(file.text("kind")),
                                          removeLabel: "Remove attachment \(file.text("name"))") {
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
                        if !selected.isEmpty {
                            AssistantChip(text: "\(selected.count) selected \(selected.count == 1 ? "transaction" : "transactions")", symbol: "checklist",
                                          removeLabel: "Clear selected transactions", onTap: { contextShown = true }) {
                                selected = []
                            }
                        }
                    }
                    .padding(.horizontal, 12)
                }
            }
            HStack(alignment: .bottom, spacing: 8) {
                Menu {
                    Button("Choose file", systemImage: "doc") { choosingFiles = true }
                    Button("Photo library", systemImage: "photo.on.rectangle") { choosingPhotos = true }
                } label: {
                    composerIcon("paperclip")
                }
                .disabled(controller.running || controller.attachments.count >= AssistantAttachments.limit || controller.busy.contains("attach"))
                .accessibilityLabel("Attach statement, document, or image")
                Menu {
                    Section("Sent with your message: \(Dates.monthLabel(store.month))") {
                        Button("Select transactions…", systemImage: "checklist") { contextShown = true }
                        if !selected.isEmpty {
                            Button("Clear selected transactions", systemImage: "xmark.circle", role: .destructive) { selected = [] }
                        }
                    }
                } label: {
                    composerIcon(selected.isEmpty ? "text.badge.plus" : "text.badge.checkmark")
                }
                .disabled(controller.running)
                .accessibilityLabel("Add budget context")
                TextField(controller.ready || controller.status == nil ? "Ask about your budget" : "Set up the assistant to send messages",
                          text: $message, axis: .vertical)
                    .lineLimit(1...6)
                    .focused($composing)
                    .padding(.horizontal, 14)
                    .padding(.vertical, 9)
                    .background(Color.cardBackground, in: RoundedRectangle(cornerRadius: 20, style: .continuous))
                    .overlay(RoundedRectangle(cornerRadius: 20, style: .continuous).strokeBorder(Color.secondary.opacity(0.25)))
                    .onChange(of: message) { _, value in if value.count > 20_000 { message = String(value.prefix(20_000)) } }
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

/// Your message: a bubble at the trailing edge, with its attachments under it.
private struct AssistantUserBubble: View {
    var entry: JSON

    var body: some View {
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
            }
        }
        .accessibilityElement(children: .combine)
        .accessibilityLabel("You: \(entry.text("text"))")
    }
}

/// The assistant's reply in Markdown, with copy, share and report under it.
private struct AssistantReply: View {
    var text: String
    var blocks: [AssistantMarkdownBlock]
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
}

/// The reply as it streams in, unformatted until it's complete, with the model's thinking folded away.
private struct AssistantLiveReply: View {
    var controller: AssistantController
    var proxy: ScrollViewProxy
    var following: Bool
    @State private var thinkingShown = false

    var body: some View {
        let reply = controller.streaming
        let thinking = controller.thinking
        VStack(alignment: .leading, spacing: 8) {
            if !thinking.isEmpty {
                DisclosureGroup(isExpanded: $thinkingShown) {
                    Text(thinking).font(.footnote).foregroundStyle(Color.secondary).frame(maxWidth: .infinity, alignment: .leading)
                } label: {
                    Label("Thinking", systemImage: "ellipsis.bubble").font(.footnote.weight(.semibold)).foregroundStyle(Color.secondary)
                }
            }
            if !reply.isEmpty {
                Text(reply).frame(maxWidth: .infinity, alignment: .leading)
            }
        }
        .padding(.vertical, reply.isEmpty && thinking.isEmpty ? 0 : 7)
        .onChange(of: reply) { _, _ in
            if following { proxy.scrollTo("bottom", anchor: .bottom) }
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

extension String {
    fileprivate var nilIfEmpty: String? { isEmpty ? nil : self }
}
