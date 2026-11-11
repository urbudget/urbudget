import SwiftUI

/// Available as a solid pill: green, amber, red or grey.
struct AvailablePill: View {
    var text: String
    var status: String
    /// The figure's size: 14 in the budget's column, where SF's digits run wider than Android's.
    var size: CGFloat = 15

    var body: some View {
        let colors = StatusColors(status)
        HStack(spacing: 4) {
            if status == "credit" { Image(systemName: "creditcard.fill").font(.system(size: 11, weight: .bold)) }
            Text(text).font(.sized(size, weight: .bold)).monospacedDigit().lineLimit(1)
        }
        .foregroundStyle(colors.content)
        .padding(.horizontal, 10)
        .frame(minHeight: 30)
        .background(colors.fill, in: Capsule())
        .fixedSize()
    }
}

/// A target's thin bar: amber while short, a lighter green once funded, red when overspent.
struct TargetBar: View {
    var progress: Int
    var status: String

    var body: some View {
        let fill: Color = status == "negative" ? .urNegative : progress >= 100 ? .urPositive.opacity(0.55) : .urWarning
        Capsule().fill(Color.primary.opacity(0.08))
            .frame(height: 4)
            .overlay(alignment: .leading) {
                GeometryReader { proxy in
                    Capsule().fill(fill).frame(width: proxy.size.width * CGFloat(min(100, max(0, progress))) / 100)
                }
            }
            .accessibilityLabel("Target \(progress)% funded")
    }
}

/// Uncleared is a dashed ring, cleared a green disc with a tick, reconciled a lock.
struct ClearedMark: View {
    var status: String

    var body: some View {
        Group {
            switch status {
            case "cleared":
                Image(systemName: "checkmark.circle.fill").font(.system(size: 21)).foregroundStyle(Color.urPositive)
                    .accessibilityLabel("Cleared")
            case "reconciled":
                Image(systemName: "lock.circle.fill").font(.system(size: 21)).foregroundStyle(Color.urPositive.opacity(0.75))
                    .accessibilityLabel("Reconciled")
            default:
                Circle().strokeBorder(Color.secondary, style: StrokeStyle(lineWidth: 1.4, dash: [2.6, 2.6]))
                    .frame(width: 19, height: 19)
                    .accessibilityLabel("Uncleared")
            }
        }
        .frame(width: 28, height: 28)
    }
}

struct FlagDot: View {
    var color: String
    var size: CGFloat = 10
    var body: some View { Circle().fill(Color.flag(color)).frame(width: size, height: size) }
}

/// A category under a transaction: a grey chip, yellow while it still needs a category.
struct TagChip: View {
    var text: String
    var attention = false

    var body: some View {
        Text(text)
            .font(.subheadline.weight(.semibold))
            .lineLimit(1)
            .foregroundStyle(attention ? Color.urOnAttention : .primary)
            .padding(.horizontal, 8)
            .frame(minHeight: 24)
            .background(attention ? Color.urAttention : Color.urZero, in: RoundedRectangle(cornerRadius: 7, style: .continuous))
    }
}

/// A notice: amber for attention, red for a problem, blue for information, green for good news.
struct Notice: View {
    var text: String
    var tone = "attention"
    var icon = "info.circle.fill"
    var action: String?
    /// A second, quieter action before the main one.
    var secondary: Secondary?
    var onAction: (() -> Void)?

    struct Secondary {
        var label: String
        var symbol: String?
        var action: () -> Void
    }

    private var ink: Color {
        switch tone {
        case "negative": return .urNegative
        case "positive": return .urPositive
        case "info": return .urAccent
        default: return .urWarning
        }
    }

    var body: some View {
        // What it says and its main action on one line; a second action has the line under them,
        // as on Android, so the words keep their room on a narrow screen.
        VStack(alignment: .leading, spacing: 10) {
            HStack(alignment: .center, spacing: 10) {
                Image(systemName: icon).foregroundStyle(ink).font(.body.weight(.semibold))
                Text(text).font(.subheadline.weight(secondary == nil ? .regular : .semibold)).foregroundStyle(Color.primary)
                    .frame(maxWidth: .infinity, alignment: .leading)
                if let action, let onAction {
                    Button(action, action: onAction).font(.subheadline.weight(.bold)).foregroundStyle(ink).buttonStyle(.borderless)
                }
            }
            if let secondary {
                Button(action: secondary.action) {
                    HStack(spacing: 6) {
                        if let symbol = secondary.symbol { Image(systemName: symbol) }
                        Text(secondary.label)
                    }
                    .font(.subheadline.weight(.semibold)).foregroundStyle(Color.urAccent)
                }
                .buttonStyle(.borderless)
            }
        }
        .padding(.horizontal, 14)
        .padding(.vertical, 12)
        .background(ink.opacity(0.12), in: RoundedRectangle(cornerRadius: 14, style: .continuous))
    }
}

struct FormErrorView: View {
    var message: String?
    var body: some View {
        if let message {
            Label(message, systemImage: "exclamationmark.circle.fill")
                .font(.footnote)
                .foregroundStyle(Color.urNegative)
        }
    }
}

/// A label over a figure, for summary strips.
struct Figure: View {
    var label: String
    var value: String
    var color: Color = .primary
    var alignment: HorizontalAlignment = .leading

    var body: some View {
        VStack(alignment: alignment, spacing: 2) {
            Text(label).font(.footnote).foregroundStyle(Color.secondary)
            Text(value).figure(17, weight: .semibold).foregroundStyle(color).lineLimit(1).minimumScaleFactor(0.7)
        }
    }
}

/// A row of a form or detail list: a title with a value at the end.
struct ValueRow: View {
    var title: String
    var value: String
    var subtitle: String?
    var color: Color = .primary

    var body: some View {
        HStack {
            VStack(alignment: .leading, spacing: 2) {
                Text(title)
                if let subtitle { Text(subtitle).font(.footnote).foregroundStyle(Color.secondary) }
            }
            Spacer(minLength: 12)
            // A figure is never broken over two lines; its title gives way.
            Text(value).monospacedDigit().foregroundStyle(color).lineLimit(1).layoutPriority(1)
        }
    }
}

/// An empty list's message, with what to do next.
struct EmptyState: View {
    var title: String
    var detail: String
    var symbol = "dollarsign.circle"
    var action: String?
    var onAction: (() -> Void)?

    var body: some View {
        ContentUnavailableView {
            Label(title, systemImage: symbol)
        } description: {
            Text(detail)
        } actions: {
            if let action, let onAction { Button(action, action: onAction).buttonStyle(.borderedProminent) }
        }
    }
}

/// A form's text field for money, in the budget's format: its label at the start and the amount at
/// the end, as Settings lays out a value, so the label stays when the field has an amount in it.
/// `labeled: false` is the bare field, for a split line's amount.
struct AmountField: View {
    var title: String
    @Binding var text: String
    var labeled = true

    var body: some View {
        if labeled {
            LabeledContent {
                // Room for the amount; the label keeps the rest of the row.
                TextField(title, text: $text, prompt: Text(Money.editable(0)))
                    .decimalKeyboard().monospacedDigit().multilineTextAlignment(Self.endAlignment)
                    .amountDirection()
                    .frame(maxWidth: 150)
            } label: {
                Text(title).layoutPriority(1)
            }
        } else {
            TextField(title, text: $text).decimalKeyboard().monospacedDigit()
                .multilineTextAlignment(Self.startAlignment).amountDirection()
        }
    }

    // A typed amount reads left to right in every language ("-12.50", never "12.50-"), so in Arabic
    // the field is laid out left to right and its alignment swapped to stay at the row's end or start.
    private static var endAlignment: TextAlignment { AppLanguage.isRightToLeft ? .leading : .trailing }
    private static var startAlignment: TextAlignment { AppLanguage.isRightToLeft ? .trailing : .leading }
}

extension View {
    /// Amounts and sums are typed left to right, also in right-to-left languages.
    func amountDirection() -> some View { environment(\.layoutDirection, .leftToRight) }
}

/// A month as a form row that opens the month grid.
struct MonthRow: View {
    var title: String
    @Binding var month: String
    @State private var picking = false

    var body: some View {
        Button { picking = true } label: {
            HStack {
                Text(title).foregroundStyle(Color.primary)
                Spacer()
                Text(Dates.monthLabel(month)).foregroundStyle(Color.secondary)
            }
        }
        .sheet(isPresented: $picking) { MonthPickerSheet(month: month) { month = $0; picking = false } }
    }
}

/// A date as a form row, in local time as the budget's day.
struct DayPicker: View {
    var title: String
    @Binding var iso: String

    var body: some View {
        DatePicker(title, selection: Binding(get: { Dates.pickerDate(iso) }, set: { iso = Dates.fromPicker($0) }), displayedComponents: .date)
    }
}

/// Runs `action` with the error shown in place of a crash or a silent failure.
func attempt(_ message: Binding<String?>, _ action: () throws -> Void) {
    do {
        try action()
        message.wrappedValue = nil
    } catch {
        message.wrappedValue = BudgetStore.readable(error)
    }
}

/// Runs a change that saves, with `saving` set meanwhile; a sheet closes in `action` once it's saved,
/// and stays open with the error if it isn't.
@MainActor func attempt(_ message: Binding<String?>, saving: Binding<Bool>, _ action: @escaping @MainActor () async throws -> Void) {
    guard !saving.wrappedValue else { return }
    saving.wrappedValue = true
    message.wrappedValue = nil
    Task { @MainActor in
        do {
            try await action()
        } catch {
            message.wrappedValue = BudgetStore.readable(error)
        }
        saving.wrappedValue = false
    }
}

/// A sheet's Save button, a spinner while it saves.
struct SaveButton: View {
    var title = String(localized: "Save")
    var saving: Bool
    var action: () -> Void

    var body: some View {
        if saving {
            ProgressView()
        } else {
            Button(title, action: action).fontWeight(.semibold)
        }
    }
}

/// A plain list's section header as a band across the screen, edge to edge with a hairline above
/// and below, as Android draws a group in the budget and a day in the register. It's solid, so
/// rows passing under it while it's pinned don't show through.
private struct ListBand: ViewModifier {
    var minHeight: CGFloat

    func body(content: Content) -> some View {
        content
            .textCase(nil)
            .padding(.horizontal, 16)
            .padding(.vertical, 6)
            .frame(maxWidth: .infinity, minHeight: minHeight, alignment: .leading)
            .background(Color.urBand)
            .overlay(alignment: .top) { Hairline() }
            .overlay(alignment: .bottom) { Hairline() }
            .listRowInsets(EdgeInsets())
    }
}

/// A line one pixel thick across its container (a `Divider` in an overlay stands upright).
struct Hairline: View {
    @Environment(\.displayScale) private var scale

    var body: some View {
        Rectangle().fill(Color.urHairline).frame(maxWidth: .infinity).frame(height: 1 / max(scale, 1))
    }
}

extension View {
    func listBand(minHeight: CGFloat = 40) -> some View { modifier(ListBand(minHeight: minHeight)) }
}

/// A form set as closely as the app's lists (`groupedList`): sections a little apart, the first
/// just under the bar.
struct CompactForm<Content: View>: View {
    @ViewBuilder var content: Content

    var body: some View {
        Form { content }
            #if os(iOS)
            .listSectionSpacing(.compact)
            .contentMargins(.top, 8, for: .scrollContent)
            .clearOfSideBar()
            #endif
            .scrollIndicators(.hidden)
    }
}

/// A sheet's one main action as a filled button across the page, on the page rather than in a
/// card, as Android's forms end (Move, Assign to a category).
private struct PrimaryAction: ViewModifier {
    func body(content: Content) -> some View {
        content
            .buttonStyle(.borderedProminent)
            .buttonBorderShape(.capsule)
            .controlSize(.large)
            .listRowBackground(Color.clear)
            .listRowInsets(EdgeInsets())
            .listRowSeparator(.hidden)
    }
}

extension View {
    func primaryAction() -> some View { modifier(PrimaryAction()) }
}
