<!--
UrBudget's app guide, which the assistant reads through get_app_guide (guide.rs).
Each entry: "## id: Title", then keywords:, what:, desktop:, android: and ios:. A field runs on
over the lines after it, so lists stay lists. Put every on-screen label in **bold**, exactly as
that app shows it in English: tests check each one against the app's own text, so a renamed
button fails a test. When an app doesn't have the feature, say "Not available on <app>." and,
if another app has it, which one. Describe icon-only buttons in words, never in bold.
-->

## map: How the app is laid out
desktop: The left sidebar has the budget switcher at the top. Click the budget's name to switch budgets, or choose **New budget…** or **Manage budgets…** to open the library. Below it are **Budget**, **Reports**, **Payees & rules** and **All transactions**, then your accounts under **Accounts**, grouped into **Budget**, **Tracking** and **Closed**. **Settings & data** is at the bottom, and Ctrl+B / ⌘B collapses the sidebar. Sync is in **Settings & data** → **Account & sync**; once you're signed in, the sync chip at the top right also opens it. The **Assistant** button at the top right (Ctrl+J / ⌘J) sits next to the undo and redo arrows.
android: The bottom bar has five tabs: **Budget**, **Transactions**, **Accounts**, **Reports** and **More**. A floating **Transaction** button above the bar adds a transaction on Budget, Transactions and Accounts. **More** opens a sheet with:
- the current budget's card at the top, which opens **Budget library**
- **Scheduled**, **Payees & rules**, **Bank connections**, **Bank messages**, **Account & sync** and **Assistant**
- **Import bank file** and **Import from YNAB**
- **Settings & data** at the bottom
Every tab's top bar has a chat-bubble icon that slides up the **Assistant** panel, and a ⋯ menu with the sync status (when signed in), **Undo**, **Redo** and **Settings & data**. **Settings & data** also links to **Budget library** and **Account & sync**.
ios: There are five tabs along the bottom: **Budget**, **Transactions**, **Accounts**, **Reports** and **More**. **More** holds the open budget (its row says "Switch or manage budgets" and opens the **Budget library**), then **Account & sync** and **Assistant**. Under **Planning** it has **Scheduled**, **Loan planner**, **Payees & rules** and **Needs a category**; under **Bring in transactions** it has **Bank connections**, **Import bank file** and **Import from YNAB**; **Settings & data** is last. The ⋯ menu (top right on Budget, Transactions, Accounts and Reports, but not More) has **Undo**, **Redo**, **Assistant**, **Sync now** (once sync is on), **Account & sync** (shown as the sync status once you're signed in) and **Settings & data**. On iPad with iOS 18 or later the tabs can open as a sidebar, and ⌘1–⌘5 switch tabs.

## assign-money: Assign money to categories
keywords: assign, budget, ready to assign, unassigned, give money, fund, allocate
what: Income lands in Ready to Assign; assigning gives it a job in a category for the month.
desktop: On **Budget**, click the **Unassigned** box above the categories. Enter an **Amount**, pick the category under **To**, and click **Assign**. You can also type straight into a category's **Assigned** cell: math like 10+2.50 works, and Enter saves and moves to the next category.
android: On the **Budget** tab, tap a category's Assigned amount, type the amount on the keypad (or tap one of the shortcut chips above it) and tap **Assign**. You can also tap the **Unassigned** banner → **Assign to a category**. In the **Assign money** sheet tap **To** to pick the category, tap the amount to change it, tap **Done**, then tap **Assign**.
ios: On **Budget**, tap a category's Assigned amount (or swipe right on the category and tap **Assign**), type the amount on the keypad, then tap **Assign**. To give money out of Unassigned instead, tap the green **Unassigned** banner (**Tap to assign**) → **Assign to a category**. Choose the category under **To**, tap the amount to change it (finish with **Done**), then tap **Assign**.

## move-money: Move money between categories or cover overspending
keywords: move, cover, overspent, overspending, take money, shift, reallocate, negative available
what: Moves part of one category's available money to another, or covers a category that went negative.
desktop: On **Budget**, click a category's Available amount and choose **Move to…** or **Take from…**. Enter the **Amount**, pick the other category or Unassigned, and click **Move**. To cover overspending, click the red Available amount (it opens on **Cover from…**), choose where the money comes from, and click **Move**.
android: On the **Budget** tab, tap a category's Available pill. Or open the category and tap **Move money** (**Cover** when it's overspent). The sheet opens as **Move money** or **Cover overspending**. Tap **From** and **To** to pick categories (or Unassigned), tap the amount, type it and tap **Done**, then tap **Move** (**Assign** when the money comes from Unassigned). The overspent-categories card's **Show** lists every overspent category.
ios: Tap a category's Available pill, or swipe left on the category and tap **Move** (it says **Cover** when the category is overspent); long-pressing a category also offers **Move money** or **Cover overspending**. In the sheet, tap the amount to type it, set **From** and **To** (**Swap** flips them), then tap **Move**. Covering overspending starts with the overspent amount and takes it from **Unassigned** when there's money there, in which case the button reads **Assign**.

## quick-assign: Quick assign
keywords: quick assign, auto assign, automatic, fill targets, underfunded, last month, average
what: Fills categories in one go: what their targets need, what was assigned or spent last month, an average, and similar options, with a preview first.
desktop: On **Budget**, use the **Quick assign** panel on the right. With nothing selected it applies to every visible category; select categories first (click, Ctrl+click / ⌘-click, Shift+click) to limit it to them. Click an option such as **Underfunded**, **Assigned last month**, **Spent last month**, **Average assigned**, **Average spent**, **Reduce overfunding**, **Reset available amounts** or **Reset assigned amounts**, check the preview, and click the Apply button. If the panel isn't showing, widen the window or close the assistant.
android: Tap the **Unassigned** banner on the **Budget** tab and scroll to **Quick assign**. Pick an option; each row shows what it would change:
- **Underfunded**, **Assigned last month**, **Spent last month**
- **Average assigned**, **Average spent**
- **Reduce overfunding**, **Reset available amounts**, **Reset assigned amounts**
Check the preview, then tap the apply button at the bottom (it says how many categories change). For a single category, tap its Assigned amount, use the shortcut chips above the keypad (needed, last month, averages), then tap **Assign**.
ios: Tap the **Unassigned** banner and scroll to the Quick assign options. They are **Underfunded**, **Assigned last month**, **Spent last month**, **Average assigned**, **Average spent**, **Reduce overfunding**, **Reset available amounts** and **Reset assigned amounts**, and each shows what it would change. Tap one to see its preview, then tap the "Apply to N categories" button at the bottom. For one group, tap the ⋯ button beside the group's name → **Quick assign…**; for one category, open its Assigned keypad and tap a suggestion chip such as Needed, Last month or Avg spent.

## targets: Set a category target
keywords: target, goal, savings goal, monthly, needed, by date, snooze, funding
what: A target says how much a category needs (each month, by a date, or a balance to keep); the budget shows what's still needed. A target can be snoozed for a month.
desktop: On **Budget**, click a category, then click **Create target** or **Edit target** in the right panel. You can also right-click the category and choose **Create target…** or **Edit target…**. Enter the **Amount**, choose **Target behavior** and **Repeats** (and a due date or start month if asked), and click **Save target**. **Remove target** deletes it. To snooze a target, open **Edit target**, tick **Snooze target this month** and click **Save target**. Snoozing is only offered while you're viewing the current month, and not for credit card payment categories.
android: Tap a category on the **Budget** tab, then tap **Create target** (or **Edit target**, or its **Target** row). Type the amount and choose one of:
- **Set aside each period**
- **Refill up to each period**
- **Maintain a category balance**
- **Reach a balance by a date**
Set **Repeats**, **Starts** (or the date) and tap **Save**. **Remove target** deletes it. To snooze, open **Edit target** while viewing the current month, turn on **Snooze this month** and tap **Save**.
ios: Tap a category → **Create target** (or **Edit target**); you can also long-press the category → **Target…**. Type the amount on the keypad, then choose **Set aside each period**, **Refill up to each period**, **Maintain a category balance** or **Reach a balance by a date**. Set **Repeats** and any dates, then tap **Save**. To snooze an existing target for the current month, turn on **Snooze this month** and tap **Save**; **Remove target** deletes the target.

## categories: Add, rename, hide or delete a category
keywords: category, add category, new category, rename, hide, delete category, remove
what: Categories are the envelopes money is assigned to. Deleting one moves its history to another category.
desktop: To add a category, click the + on a group's row (or right-click the group → **Add category**), type the name and press Enter. To rename, double-click the name, press F2, or right-click → **Rename**. To hide, right-click → **Hide category**; it moves to **Hidden categories**, where **Show category** brings it back. To delete, right-click → **Delete…**, choose where its transactions and money go, and click **Delete category**. The … button at the end of the row opens the same menu.
android: To add a category on the **Budget** tab, tap the filter icon (top left) → **New category**, or use a group's ⋯ → **Add category**. Enter **Category name**, pick the **Group** and tap **Add**. To change a category, tap it, then use its ⋯ menu:
- **Name, group & note**: edit **Name** or switch on **Hidden**, then tap **Save**.
- **Hide category** / **Show category**.
- **Delete category**: if it has history, choose where to **Move its history into** and tick **Delete this category and move its history**, then tap **Delete**.
Hidden categories come back into view with filter icon → **Show hidden categories**.
ios: To add one, tap the + button at the top right of **Budget** → **New category** (or tap **Category** at the bottom of the list, or a group's ⋯ → **Add category**). Enter the **Category name**, pick the **Group** and tap **Add**. To rename or hide, long-press the category → **Name, group & note…** (edit **Name**, or turn on **Hidden**, then **Save**) or tap **Hide category**; to bring a hidden one back, use the filter button at the top left → **Show hidden categories**, then long-press it → **Show category**. To delete, long-press → **Delete category…**, choose where its history goes under **Move its history into**, turn on **Delete this category and move its history**, then tap the red "Delete <name>" button.

## groups: Add, rename, reorder or delete a category group
keywords: group, category group, new group, rename group, merge, delete group
what: Groups hold related categories and show their totals.
desktop: To add a group, click **Group** in the toolbar above the categories, type the name and press Enter. To rename, double-click the group name, press F2, or right-click it → **Rename group**; renaming it to an existing group's name merges the two. To reorder, drag the group by its grip (in the **All** view) or press Alt+↑ / Alt+↓ (⌥↑ / ⌥↓ on Mac) on the group name. To delete, right-click → **Delete group…**, choose where its categories go, and click **Delete group**.
android: To add a group: **Budget** tab → filter icon (top left) → **New group**, type the **Group name** and tap **Add**. Everything else is in the ⋯ next to the group's name:
- **Rename group**: type a new **Group name** and tap **Save**. Using an existing name merges the two groups.
- **Move up** / **Move down**
- **Hide all categories**
- **Delete group…**: choose the **Group** its categories move to, then tap **Delete**.
**Move up**, **Move down** and **Hide all categories** only appear when the group has categories.
ios: To add one, tap + → **New category group** (or **Category group** at the bottom of Budget), type the **Group name** and tap **Add**. For everything else, tap the ⋯ button beside the group's name. **Rename group** → **Save** renames it (typing an existing group's name merges the two); **Move up** and **Move down** reorder it, or use the filter button → **Reorder groups**, drag the handles and tap **Save**. **Delete group…** asks which group its categories move to, then tap **Delete**.

## reorder-categories: Reorder categories or move one to another group
keywords: reorder, order, move category, drag, sort, another group
what: Changes the order categories appear in, or which group a category belongs to.
desktop: On **Budget**, in the **All** view, drag a category by its grip or its name to a new spot. Drop it on another group's header row to move it into that group. From the keyboard, focus the category name and press Alt+↑ / Alt+↓ (⌥↑ / ⌥↓ on Mac). You can also select the category, click **Name, group and visibility…**, change **Group** and click **Update category**.
android: Tap the category on the **Budget** tab → ⋯ → **Move or reorder**. Choose its **Group**, where to **Place before** (or **End of group**), optionally **Place group before**, and tap **Save**. There is no drag-to-reorder; groups move with **Move up** / **Move down** in the group's ⋯ menu.
ios: Tap the filter button at the top left of **Budget** → **Reorder categories**, drag the handles to reorder categories within their group, then tap **Done**. To move a category to another group, long-press it → **Move to another group**, pick the **Group** (or **New group…**) and tap **Move**. You can also change the **Group** under **Name, group & note…**.

## month-navigation: Look at another month
keywords: month, previous month, next month, change month, other month, go to month
what: The budget is per month; earlier and later months can be opened and planned.
desktop: On **Budget**, use the left and right arrows beside the month name at the top. You can also click the calendar button next to them and pick a month; **This month** in that picker jumps back. When you're viewing another month, click **Today** to return to the current one.
android: On the **Budget** tab, use the arrows either side of the month at the top. You can also tap the month name to open the month picker: its arrows change the year, and you tap a month or **Go to this month**. When you're on another month, a pill above the Unassigned banner jumps back to the current month. The **Reports** overview has the same month picker.
ios: On **Budget**, tap the arrows on either side of the month name at the top, or tap the month name to open the **Month** grid, where arrows change the year, and pick a month; **Go to this month** jumps back. Away from the current month, a "Back to <month>" button also appears above the Unassigned banner. **Reports** has its own month arrows at the top of its overview, and they change the same month.

## notes: Category and month notes
keywords: note, notes, memo, comment, month note, category note
what: A note can be kept on a category, or on a month.
desktop: For a category note, click the category on **Budget** and type in **Notes** in the right panel; it saves when you click away. You can also use **Name, group and visibility…** → **Category notes** → **Update category**, and hovering over the category name shows the note. For a month note, type in the field in the toolbar above the categories (it reads "Note for <month>…"); it saves when you pause or press Enter.
android: Category note: tap the category → **Add a note** (it reads **Note** once there is one), type it and tap **Save**. It can also be edited in ⋯ → **Name, group & note**. Month note: **Budget** tab → filter icon (top left) → **Month note & income**, type in **Month note** and tap **Save**. The note then shows above the categories, and tapping it reopens the editor.
ios: For a category note, tap the category → under **Note** tap **Add a note** (or the existing note), type it and tap **Save**; it's also editable in **Name, group & note…**, and categories with a note show a note icon. For a month note, tap the filter button → **Month note & income** (or the **Unassigned** banner → **Month note**), type in the **Month note** section and tap **Save**. The month note then shows above your categories, and tapping it reopens the note.

## budget-views: Filter the budget or find a category
keywords: view, filter, underfunded, overspent, overfunded, snoozed, search category, find category
what: The budget can show only some categories, such as underfunded or overspent ones, and categories can be searched.
desktop: On **Budget**, use the tabs above the categories: **All**, **Underfunded**, **Overfunded**, **Money available**, **Overspent** and **Snoozed**. Each tab shows a count and appears only when that month has categories that fit it. **Collapse all** and **Expand all** fold the groups. There is no category search box on the Budget page.
android: On the **Budget** tab, tap the filter icon at the top left and pick **All**, **Underfunded**, **Overfunded**, **Money available**, **Overspent** or **Snoozed** (each shows a count); **Show all** returns to the full list. The same menu has **Show hidden categories** and **Expand all** / **Collapse all**, and **Show** on the overspent-categories card opens the Overspent view. The Budget tab has no category search; the category pickers (such as Move money's **From**/**To**) have a **Search categories** field.
ios: Tap the filter button at the top left of **Budget** and pick **All**, **Underfunded**, **Overfunded**, **Money available**, **Overspent** or **Snoozed**; each shows its count. **Show all** returns to everything, and the overspent notice's **Show** jumps straight to **Overspent**. The same menu has **Show hidden categories** and **Collapse all** / **Expand all**. To search, pull the list down to reveal **Find a category** and type.

## expected-income: Set expected income
keywords: expected income, income, salary, plan income, expected
what: Expected income lets the month's plan compare targets with the money that's coming.
desktop: On **Budget**, clear any category selection (press Esc) so the right panel shows **This month**. Under **Targets and income**, type the amount in **Expected income** and press Enter; the panel then compares it with what the month's targets ask for. If the panel is hidden, widen the window or close the assistant.
android: Tap the **Unassigned** banner on the **Budget** tab → under **Targets & income** tap **Expected income**. Or use the filter icon (top left) → **Month note & income**. Enter the amount in the expected monthly income field and tap **Save**. Leaving it blank clears it; it never adds money to Unassigned.
ios: Tap the **Unassigned** banner → **Expected income** (in **Targets & income**), or tap the filter button → **Month note & income**. Enter the amount in the expected monthly income field and tap **Save**. The Unassigned sheet then compares it with this month's targets; it doesn't add money to Unassigned.

## add-transaction: Add a transaction
keywords: add transaction, new transaction, enter, record, spend, purchase, expense, income, payment
what: A transaction records money in or out of an account, with a payee and a category.
desktop: Open an account in the sidebar (or **All transactions**) and click **Add transaction**, or press N. Fill in the new row (date, payee, category, memo, and Outflow or Inflow). Click **Save** (Enter), or **Save & add another** (Shift+Enter / ⇧Enter); **More options** opens the full form.
android: Tap the floating **Transaction** button at the bottom right of **Budget**, **Transactions** or **Accounts**. In **New transaction**:
1. Choose **Outflow** or **Inflow**, type the amount on the keypad and tap **Done**.
2. Set **Payee** (type to search, or type a new name to add it), then **Category**, **Account** and **Date**.
3. Optionally set **Repeat**, **Memo** and **Flag**.
4. Tap **Save** (it reads **Schedule** when the date is in the future).
ios: Tap the floating **Transaction** button (with the +) at the bottom right of **Budget**, **Transactions** or **Accounts**. Choose **Outflow** or **Inflow**, type the amount on the keypad and tap **Done**; then set **Payee**, **Category**, **Account** and **Date**, and tap **Save** (or **Save & add another**). From the Home Screen, long-press the app icon → **New transaction**, or say "Add a transaction in UrBudget" to Siri and use the **Add a transaction** shortcut from Shortcuts, Spotlight or the Action button.

## split-transaction: Split a transaction across categories
keywords: split, multiple categories, divide, several categories, split transaction
what: One transaction can be divided between several categories.
desktop: In the transaction row you're adding or editing, click **Split**, then give each line a category and an amount. Click **Add line** for more lines until it says **Fully split**, then click **Save**. In the full form (**More options**), click **Split transaction** to add category lines; their amounts must add up to the total.
android: In the transaction editor, tap **Split between categories** (in the second card, or at the top of the category picker). For each line tap **Choose category** and enter its **Amount** (and **Memo**). Tap **Add line** for more lines, or use the "Use the … left" button to fill in the rest. Tap **Save** once it shows **Fully split**. **Undo split** appears when only one line is left.
ios: In the transaction editor, tap **Split between categories** (it's also at the top of the **Category** picker). Give each line a category and amount, tap **Add line** for more, and save once the header reads **Fully split**; the "Use the … left" button fills in the remainder. **Undo split** appears when only one line is left.

## transfer: Record a transfer between accounts
keywords: transfer, move between accounts, savings, transfer money, between accounts
what: A transfer moves money between two of your accounts; it needs no category when both are budget accounts.
desktop: Add a transaction and, in the payee field, pick the "Transfer: <account>" suggestion for the other account; enter the amount and click **Save**. Or open the full form (**More options**), click **Transfer**, pick **From account** and **To account**, and click **Add transaction**. For a credit card payment, **Make payment** on the card's register is quicker.
android: Tap the **Transaction** button and enter the amount: **Outflow** sends money from this account, **Inflow** brings it in. Tap **Payee**, pick the other account under **TRANSFER TO OR FROM**, set **Account** to this side's account, and tap **Save**. A transfer between two budget accounts needs no category, and the swap icon on a transfer's row opens the other account.
ios: Start a new transaction, tap **Payee**, and pick the other account under **Transfer to or from**. Enter the amount (**Outflow** means money leaves this account) and tap **Save**. A category is only asked for when the transfer is between a budget account and a tracking account; to jump to the other side later, long-press the transfer → "Go to <account>".

## edit-delete-transaction: Edit, duplicate or delete a transaction
keywords: edit, change transaction, duplicate, copy, delete transaction, remove transaction
what: Opens a transaction to change it, make a copy, or remove it.
desktop: To edit, double-click a transaction (or select it and press Enter or F2, or click it again), change the fields and click **Save**; **More options** opens the full **Edit transaction** form. To duplicate, select it and press Ctrl+D / ⌘D, or click the three-dot button in the bar at the bottom → **Duplicate**. To delete, select it and press Delete, or click the trash-can button in the bottom bar; the message that appears has **Undo**.
android: Tap a transaction to open **Edit transaction**, change it and tap **Save**. To remove it, tap **Delete transaction** at the bottom → **Delete** (both sides of a transfer go). To duplicate, long-press it to select it, tap **More** in the bottom bar, set **Change** to **Duplicate** and tap **Apply**.
ios: Tap a transaction to open **Edit transaction**, change it and tap **Save**; **Delete transaction** → **Delete** removes it, along with both sides of a transfer. Long-press a transaction for **Edit**, **Duplicate** or **Delete**, or swipe left → **Delete**; each change shows an **Undo** message.

## approve-clear: Approve, clear or flag transactions
keywords: approve, unapproved, cleared, uncleared, flag, mark, imported
what: Imported transactions wait for approval; cleared marks what the bank has confirmed; flags are coloured markers.
desktop: A transaction waiting for approval has an approve button at its row's left edge; click it, or select rows and press A (or click the check button in the bottom bar). The badge beside an account in the sidebar counts them, and **Filters** → **Flag & review** → **Needs approval** lists them. To mark a transaction cleared or uncleared, click its circle in the Cleared column, or select rows and press C. To flag one, click the flag spot near its left edge and pick a flag (**No flag** removes it), or use the flag button in the bottom bar.
android: Approve imported transactions with the **Approve** button on their row. **Review** on the "transactions to approve" notice lists them all; you can also long-press to select several and tap **Approve** in the bottom bar, and the editor has **Approve** too. Tap the circle beside an amount to toggle cleared, or use the **Cleared** switch in the editor or **Cleared** in the selection bar. To flag, use the editor's **Flag** row or the selection bar's **Flag** and pick a colour or **No flag**.
ios: Imported transactions waiting for approval show a sparkle, and **Transactions** shows a banner counting them with **Review** and **Approve all**. Swipe right on a transaction for **Approve**, **Cleared** / **Uncleared** (and **Categorize** when it needs one), or long-press it for **Mark cleared** / **Mark uncleared** and the **Flag** submenu. In the editor, use **Approve**, the **Cleared** switch and the **Flag** picker.

## search-filter: Search and filter transactions
keywords: search, filter, find transaction, look for, date range, amount, payee
what: The register can be searched and filtered by text, dates, amounts, status and more.
desktop: In a register, type in the search box (press / or Ctrl+F / ⌘F to jump there). Typing payee:, category:, amount: or is:cleared turns the search into filter chips. Use the date-range button for dates, and **Filters** for **Cleared status**, **Flag & review**, a category (**Add a category**) and an **Amount** range, then click **Done**. **Clear filters** resets everything.
android: On **Transactions**, tap the magnifier icon and type in the search box (payee, category, memo or amount). Use the pills under it:
- **Filters**: **Status**, **Category**, **Flag**, **At least** / **At most**, **Sort**, **Clear filters**, then **Done**
- **Dates**: a preset, or **Custom range** → **From** / **Through** → **Apply**
- Quick toggles: **Needs approval**, **Uncleared**, **Needs a category**, **Flagged**
To see one account's transactions, open the account from **Accounts**.
ios: On **Transactions** (or any account's register), pull the list down to reveal the search field, **Payee, category, memo, amount**, and type. Tap the filter button at the top right for **Status**, **Flag**, **Dates**, **Amount and custom dates…**, **Category…**, **Needs a category** and **Sort**. A summary row lists what's active, with **Clear** to remove it all.

## bulk-edit: Change several transactions at once
keywords: bulk, select, multiple, several, many transactions, batch, categorize many
what: Select several transactions to categorize, approve, flag, change the payee or memo, duplicate or delete them together.
desktop: In a register, tick the rows' checkboxes: Shift+click selects a range, Ctrl+click / ⌘-click adds one, and Ctrl+A / ⌘A selects everything listed. The bar at the bottom approves, marks cleared, categorizes (tag button), flags, or deletes them (trash-can button). Its three-dot button adds **Mark uncleared**, **Rename payee…**, **Set memo…** and **Duplicate**. Esc clears the selection.
android: In **Transactions**, long-press a transaction to start selecting, then tap others (or **Select all**). The bottom bar offers **Approve**, **Category**, **Cleared**, **Flag**, **Delete** and **More**. **More** opens a sheet where **Change** can be **Payee**, **Memo**, **Cleared status**, **Duplicate**, **Approve** or **Delete**; set it and tap **Apply**. Tap the X or Back to end the selection.
ios: On **Transactions**, tap **Select** (or long-press a transaction → **Select**) and tap the transactions you want; **Select all** takes everything listed. The bottom bar has **Approve**, **Category**, **Cleared** and **Flag**, and **More** has **Change payee…**, **Change memo…**, **Mark uncleared**, **Duplicate**, **Ask the assistant** and **Delete**. Tap **Done** to finish; each change offers **Undo**.

## scheduled: Scheduled and recurring transactions
keywords: scheduled, recurring, repeat, subscription, bill, monthly, enter, skip, pause
what: A scheduled transaction repeats; each time it's due it can be entered, skipped, or paused.
desktop: In a register, click **Scheduled** → **New scheduled transaction**, fill it in, choose **Repeat** and click **Save schedule**. You can also give a new transaction a future date or a repeat in its row. Upcoming ones are listed under **Scheduled** at the top of the register, with **Enter** (post it now) and **Skip**. Right-click one, or click its calendar icon, for **Edit schedule…**, **Pause** / **Resume** and **Delete schedule**.
android: Create: **More** → **Scheduled** → **Add schedule** opens **New schedule** (the transaction editor plus **Repeat** and a **Paused** switch); tap **Save**. You can also set **Repeat** when adding a normal transaction. Act on schedules:
- **Enter all due** enters everything that's due.
- A schedule's ⋯ (or a long-press) has **Enter now**, **Skip it** (or Skip plus its date), **Pause** / **Resume**, **Edit** and **Delete**.
- Schedules also show at the top of **Transactions** with **Enter** and **Skip**.
**Settings & data** → **Enter due schedules on opening** enters due ones automatically.
ios: Go to **More** → **Scheduled** and tap the + button; in **New schedule**, set the amount, payee, **Repeat** and **Next date**, then tap **Save**. A new transaction with a **Repeat** or a future date also becomes a schedule when you tap **Schedule**. Swipe right on a schedule to **Enter** or **Skip** it, and swipe left to **Pause** / **Resume** or **Delete**; long-press adds **Enter now** and **Edit**, and **Enter the one that's due** / "Enter all N due" enters what's due. Schedules also appear at the top of **Transactions** with **Enter** and **Skip** buttons.

## export-csv: Export transactions to a CSV file
keywords: export, csv, spreadsheet, excel, download transactions, register export
what: Saves transactions as a CSV file to open in a spreadsheet.
desktop: Open an account (or **All transactions**) and click **Export** at the top of the page. It saves the transactions as a CSV file; if a search or filter is on, only the listed ones are saved. You can also right-click an account in the sidebar → **Export transactions**.
android: Open **Transactions**, tap the magnifier icon, then tap the **Export** pill and choose where to save the CSV (urbudget-register.csv). Reports have their own **Export CSV** pill.
ios: On **Transactions** (or inside an account), tap the filter button at the top right → **Export all transactions**, or **Export these transactions** when a search or filter is on, and choose where to save the CSV. For whole-budget files, go to **More** → **Settings & data** → **CSV exports** and tap **Register** or **Plan**.

## add-account: Add an account
keywords: add account, new account, bank account, checking, savings, credit card, opening balance, starting balance
what: Accounts hold the money: cash and checking (budget), credit cards, and tracking accounts such as loans and investments, each with a starting balance.
desktop: Click the + next to **Accounts** in the sidebar (or **Add account** on an empty budget). Enter the **Account name**, pick the **Account type**, and type today's balance in **Opening balance** (what you owe as a negative amount). Click **Add account**.
android: On **Accounts**, tap the + at the top (or **Add account** when the list is empty). Enter **Account name**, choose **Account type** (for example **Checking**, **Savings**, **Credit card** or a loan type) and enter the current balance (debts as a negative amount). Tap **Add**.
ios: On **Accounts**, tap the + button at the top right (or **Budget**'s + → **New account**). Enter the **Account name** and choose the **Account type**. Type today's balance in the current balance field (enter a debt as a negative amount), then tap **Add**.

## credit-cards: Credit card payments
keywords: credit card, card payment, pay off card, payment category, card balance
what: Card spending moves money into the card's payment category, ready for the payment; a payment is a transfer to the card.
desktop: When you add a credit card account, UrBudget creates its payment category, named after the card, in the Credit Card Payments group; it holds the money set aside to pay the card. To pay, open the card's register and click **Make payment** (or select the payment category on **Budget** and click **Make a payment**). Choose **What's set aside**, **The whole balance** or **Another amount**, pick the **From** account and **Date**, and click the Pay button.
android: Adding an account of type **Credit card** creates its payment category in the **Credit Card Payments** group automatically. Tapping that category on **Budget** shows **Set aside for the card**. To pay the card, open it from **Accounts** (or open its payment category) and tap **Make a payment**. Choose **What's set aside**, **The whole balance** or **Another amount**, check **From** and **Date**, then tap the Pay button.
ios: Adding an account of type **Credit card** creates its payment category on **Budget**, which holds the money set aside for the card. To pay the card, long-press it on **Accounts** → **Make a payment**; the button is also in the card's register and in its payment category's sheet. Choose **What's set aside**, **The whole balance** or **Another amount**, pick the account under **From**, and tap **Pay**.

## reconcile: Reconcile an account
keywords: reconcile, reconciliation, match bank balance, statement balance, adjustment, cleared balance
what: Checks the cleared balance against the bank's, adds an adjustment if they differ, and marks those transactions reconciled.
desktop: Open the account and click **Reconcile**, or right-click it in the sidebar → **Reconcile…**. Enter your bank's balance in **Bank balance** and click **Finish reconciliation**. If the two balances differ, the button becomes **Add adjustment & reconcile**, which records the difference as an adjustment.
android: Open the account from **Accounts** and tap **Reconcile** (or long-press it in **Accounts** → **Reconcile**). If the cleared balance matches your bank, tap **Yes**, then **Reconcile**. If not, tap **No**, enter **Bank's balance** and **Statement date**, tap **Review**, then tap **Adjust & finish** (or **Reconcile** if they now match).
ios: On **Accounts**, long-press the account → **Reconcile** (or open it and tap **Reconcile**). If the cleared balance matches your bank, tap **Yes** → **Reconcile**. If not, tap **No**, enter the **Bank's balance** and **Statement date**, tap **Review**, then **Adjust & finish**.

## close-account: Close, reopen or reorder accounts
keywords: close account, reopen, archive, closed, hide account, reorder accounts
what: A closed account keeps its history but leaves the active lists.
desktop: Right-click the account in the sidebar (or click its … button) → **Close account**. It moves to the **Closed** section, where **Reopen account** brings it back. To reorder accounts, drag them within their section, or use **Move up** / **Move down** in the same menu.
android: On **Accounts**, long-press the account → **Close account**; a snackbar offers **Undo**. You can also use **Edit account** → **Account closed** switch → **Save**. Closed accounts sit in the folded **Closed** section: tap its header, then long-press the account → **Reopen account**. To reorder accounts within their section, long-press → **Move up** / **Move down**.
ios: On **Accounts**, long-press an account → **Close account**; it moves to the folded **Closed** section, where a long-press → **Reopen account** brings it back. You can also open **Edit account**, turn on **Account closed** and tap **Save**. To reorder accounts, long-press one → **Move up** / **Move down**.

## loans: Loans and the loan planner
keywords: loan, mortgage, interest, rate, amortization, payoff, extra payment, loan planner, debt
what: A debt account can carry its loan terms; the loan planner shows the payoff, schedule and what extra payments save.
desktop: Add the loan as an account with a loan type such as **Mortgage**, **Auto loan** or **Personal loan**, entering what you owe as a negative balance. Open it, click **Add loan details**, fill in **Interest rate (%)**, **Monthly payment** and **Payment category**, then click **Update account**. The loan's register then has **Record payment**, **Payoff planner** (try **Extra each month**, **One-off payment now** or **Pay off by**) and **Edit loan details**. Right-click the account → **Loan details & payoff…** opens the planner too.
android: Set up the loan as a debt account (for example **Mortgage**, **Auto loan**, **Student loan** or **Personal loan**). Add its details in one of these places:
- the **Loan details** section of the account form when you create it, or
- later, open the account from **Accounts** and tap **Add loan details** (long-press → **Loan details & payoff** also works).
Fill in **Interest rate (%)**, the monthly payment, **Payment category** and any rate changes, then tap **Add** or **Save**. The loan's register then shows **Record payment**, **Payoff planner** and **Loan details**. The planner (extra each month, a one-off payment, **Pay off by a date…**) only previews; it changes nothing. Its payment category's sheet also offers **Record payment**, **Open planner** and **Use this payment as target**.
ios: A loan is a tracking account of a loan type (for example **Mortgage** or **Auto loan**) with what you owe as a negative balance. Go to **More** → **Loan planner** and tap the loan (or long-press it on **Accounts** → **Loan details & payoff**). Fill in **Interest rate (%)**, **Monthly payment**, **Monthly fees**, **Payment category** and any **Add rate change**, then tap **Save**. The **Payoff planner** tries **Extra each month**, **One-off payment now** or **Pay off by a date…** without changing anything, and **Record payment** enters a payment; a category that pays a loan also offers **Use this payment as the target**.

## credit-limit: A card's credit limit and credit left
keywords: credit limit, limit, credit left, available credit, available limit
what: A credit card account can carry its limit, so the app shows the credit left: the limit plus the balance.
desktop: Open the card and click its pencil (edit) button, or right-click it in the sidebar → **Edit account…**. Enter the **Credit limit** (leave it blank for none) and click **Update account**. The card's register then shows **Credit left**.
android: Open the card's **Edit account** (long-press it in **Accounts**, or tap **Edit account** on its register), fill in the optional credit limit field and tap **Save**. The card's register then shows **Credit left** beside **Cleared** and **Uncleared**. The assistant can also set the limit from bank texts (**Set the limit**).
ios: On **Accounts**, long-press the credit card → **Edit account**, type the credit limit (leave it blank for none) and tap **Save**. The card's register then shows "Credit left" under its **Working balance**.

## net-worth: See net worth
keywords: net worth, assets, debts, wealth, total
what: Net worth adds every account that counts toward it: assets less debts.
desktop: Click **All transactions**; **Net worth** is shown at the top. For how it changes over time, go to **Reports** → **Wealth** → **Net worth** (or **Balance sheet**). To leave a tracking account out, untick **Count this account in net worth** in **Edit account**.
android: The **Accounts** tab shows **Net worth** at the top. For the history, open **Reports** and tap the **Net worth** card, or **All reports** → **Net worth** / **Balance sheet** under **Wealth**. To leave a tracking account out, turn off **Count in net worth** in its **Edit account**.
ios: Open **Accounts**: **Net worth** is at the top, and tapping it opens the **Net worth** report month by month. It's also a card and a report on **Reports**. A tracking account can be left out by turning off **Count in net worth** in **Edit account**.

## import-bank-file: Import a bank file
keywords: import, bank file, statement, csv, ofx, qfx, qif, excel, camt, mt940, upload, file
what: Reads a bank's export (CSV, Excel, OFX, QFX, QIF, CAMT.053, MT940) into an account, with a review before anything is added.
desktop: Open the account and click **Import** at the top, or go to **Settings & data** → **Import** → **Import a bank file**. Check **Into account** and click **Choose a file** (CSV, Excel, OFX, QFX, QIF, CAMT.053 or MT940). If lines need fixing, use the picker in each column's heading to say what it holds (**Date**, **Payee**, **Amount**, **Money out**, **Money in** and so on, or **Not read**), answer any date-order or sign question, and click **Continue to the review**. Choose what each row does, then click the Import button; the imported transactions wait for your approval.
android: Go to **More** → **Import bank file** (also under **Settings & data** → **Imports**). Pick **Into account**, tap **Choose a file** (CSV, Excel, OFX, QIF, CAMT.053 or MT940), fix any lines that need attention and tap **Continue**. Decide on each row, then tap the Import button. PDF statements go through **Read it with the assistant**.
ios: Go to **More** → **Import bank file** (or open an account and tap **Import**), set **Into account**, and tap **Choose a file** (CSV, Excel, OFX, QFX, QIF, CAMT.053 or MT940). Fix or include lines under **Needs attention** and **Left out** (or tap **Fix with the assistant**), tap **Continue**, decide each row in **Review import**, then tap "Import N". Statement files shared to UrBudget from Files, Mail or AirDrop open this import too, and a PDF goes through **Read it with the assistant**.

## bank-connections: Connect a bank with SimpleFIN
keywords: simplefin, bank connection, connect bank, sync bank, link account, download transactions, bank sync
what: SimpleFIN Bridge downloads transactions from a connected bank into linked accounts, with a review first.
desktop: Go to **Settings & data** → **Import** → **Bank sync** and click **Open SimpleFIN Bridge** to add your bank and make a setup token. Paste the token in **Setup token** and click **Connect**. Under **Syncs into**, choose the UrBudget account for each bank account (and the first day to bring in), click **Sync now**, and review the rows before they're imported. After that, click **Sync** at the top of a linked account's register, or right-click it → **Sync with bank**.
android: Go to **More** → **Bank connections**:
1. Paste your **SimpleFIN setup token** (**Get a token** helps you get one) and tap **Connect**.
2. Under **Bank accounts**, tap each bank account, choose its **Budget account** and **First import from** date (optionally **Reverse bank amount signs**), then tap **Save**.
3. Tap **Download activity** and review each account: use the import / **Mark checked** button, or **Skip this account**.
**Refresh accounts**, **Unlink account** and **Disconnect this device** manage the connection.
ios: Go to **More** → **Bank connections** → **Get a setup token** (on SimpleFIN Bridge), paste the token in **SimpleFIN setup token** and tap **Connect**. Tap each bank account to link it (choose the **Budget account**, set **First import from**, tap **Save**). Then tap **Download activity** and review each account, finishing with "Import N", **Mark checked** or **Skip this account**.

## ynab-import: Import from YNAB
keywords: ynab, you need a budget, import ynab, migrate, switch from ynab
what: Brings a YNAB budget in, from YNAB's API with a token or from YNAB's export files.
desktop: Go to **Settings & data** → **Import** → **Import from YNAB** and paste your YNAB personal access token (in YNAB: Account Settings → Developer Settings → New Token). Pick the **Plan**, choose **Merge with this budget** or **Replace this budget**, and click **Import & save**. Without a token, use **Import YNAB's export file** → **Choose export file…** and pick YNAB's export .zip; an empty budget's welcome screen also offers **I'm coming from YNAB**.
android: Go to **More** → **Import from YNAB** (also under **Settings & data** → **Imports**). Under **Import method**:
- **YNAB account**: paste a **Personal access token** (optionally tick **Remember in device credential store**), tap **Load YNAB plans**, pick the **Plan** and tap **Import changes**. To replace instead, tick **Replace budget instead of merging changes** → **Import and replace**. If asked, finish with **Accept differences and continue** or **Apply chosen versions**.
- **YNAB export files**: **Choose YNAB export files** → set **Date order** → **Preview file import** → **Accept preview and replace**.
ios: Go to **More** → **Import from YNAB**. For **YNAB account**, put your token in **Paste your personal access token** (optionally **Remember on this device**), choose the **Plan**, pick **Merge with this budget** or **Replace this budget**, and tap **Import**; if competing changes come up, pick **This budget's** or **YNAB's** for each, then tap **Apply**. For **Export files**, tap **Choose export file…**, then **Create** (a new budget) or **Replace this budget…**; a YNAB .zip shared to UrBudget from Files opens here too.

## bank-texts: Bank texts (SMS)
keywords: sms, text, texts, text messages, bank messages, bank texts, message, notification
what: On Android, the app can read the texts your bank sends to this phone into a review of transactions. It's opt-in and only reads the senders you choose.
desktop: Not available on desktop. Bank texts are read on an Android phone (More → Bank messages in the Android app); the transactions added there reach this computer with Sync.
android: Set up: **More** → **Bank messages** → **Allow SMS access** → **Choose senders**. Tick your bank under **Banks and services**, set **Collect** (**New messages only**, **From a date** or **Everything on this phone**) and tap **Save**. With an assistant connected, tap the read button (**Allow** the first time), then **Review**. Tick what to add and tap the Add button, or **Later**. **Discard review** drops the review, and **Read new texts as they arrive** reads in the background. To stop, tap **Stop and clear messages** at the bottom and confirm.
ios: Not available on iPhone. iOS doesn't let apps read text messages, so bank texts are read on an Android phone (More → Bank messages in the Android app); the transactions added there reach the iPhone with Sync.

## payees-rules: Payees and payee rules
keywords: payee, payees, rule, rules, rename payee, merge payees, default category, import rule
what: Payees can be renamed or merged, and rules set a payee's name and category when transactions come in.
desktop: Click **Payees & rules**. On the **Payees** tab, pick a payee to set its **Default category** or rename it (type a new name and click **Rename**; it says **Merge** if that name already exists). You can also merge it with **Merge into another payee** → **Merge**, or add **Bank names** with **Add**. On **Import rules**, fill in **When the bank payee contains**, **Rename to** and/or **Category**, then click **Add rule**; the pencil and trash icons on a rule edit or delete it.
android: Go to **More** → **Payees & rules**.
- **Payees**: use **Search payees**, then tap a payee to set its **Default category**, change its **Name** or **Merge into another payee**, and tap **Save**. The + beside **Payees** adds one.
- **Import rules**: tap + (or an existing rule), fill **Bank payee contains**, **Rename to** and/or **Category**, and tap **Save**. An open rule also has **Move earlier** / **Move later**, **Apply to existing transactions** and **Delete rule**.
- **Apply rules to existing transactions** runs all the rules.
ios: Go to **More** → **Payees & rules**. On the Payees tab, tap a payee to change its **Name**, set a **Default category** or **Merge into another payee**, then tap **Save**. On the Import rules tab, tap + (or long-press a payee → **Add an import rule for it**), fill in **Bank payee contains**, **Rename to** and/or **Category**, and tap **Save**. Long-press a rule for **Edit rule**, **Move earlier** / **Move later**, **Apply to existing transactions** or **Delete rule**; renaming a bank payee by hand in a transaction also offers **Add rule**.

## reports: Reports
keywords: report, reports, chart, spending, income, trends, by category, by payee, calendar, cash age, forecast
what: Reports show spending, income and wealth over a range of months, with filters for accounts and categories and links to the transactions behind each figure.
desktop: Click **Reports**, choose **Spending**, **Income** or **Wealth** at the top, then a report tab such as **By category**, **Income vs. expense** or **Net worth**. Change the period with the date-range button (presets like **Last 12 months**, or **Custom range** → **Apply**), and what's included with the **Accounts** and **Categories** filter buttons. Click a bar, slice or row to list its transactions, then click **Open in register** to see them there.
android: Open **Reports** and tap an overview card (**Spending breakdown**, **Income vs. spending**, **Net worth**, **Cash age**) or a report under **All reports**. Inside a report:
- Switch between **Spending**, **Income** and **Wealth**, and use the report chips.
- Tap the range pill for **Months** (a preset, or **Custom range** → **Apply**).
- Use the **All accounts** / **All categories** pills to filter.
- Tap a category, payee, bar or day to list its transactions; tap one to edit it, or tap **Open in register**.
ios: Tap **Reports**, then a card's heading or any report in the list (for example **Spending by category**); tapping the report's title switches to another report. Filter with the chips at the top: the date chip (presets or **Custom range…**), **All accounts** and **All categories**. Tap a category, payee, day or row to see the transactions behind it, and drag across a chart to read each month.

## report-export: Export a report
keywords: export report, csv, download report, share report
what: Saves a report's figures as a CSV file.
desktop: On **Reports**, set the range and filters you want, click **Export CSV** on the filter row, and choose where to save the file. **Forecast** has no export.
android: In any report except Forecast, tap the **Export CSV** pill in the filter row and choose where to save. The file follows the report's current range, accounts and categories.
ios: Open a report and tap the share button at the top right ("Export CSV") to save a CSV of its current range and filters. You can also go to **More** → **Settings & data** → **Report CSV**, set **From** and **Through**, and tap **Export report**.

## account-sign-in: Sign in to an UrBudget account
keywords: sign in, log in, account, email code, sign out, log out, delete account
what: An UrBudget account is only needed for Sync. Signing in uses a code sent by email.
desktop: Go to **Settings & data** → **Account & sync**, type your **Email** and click **Email me a code**. Enter the 6-digit code and click **Sign in**. To sign out, click **Sign out** there and confirm with **Sign out**. To delete your account, click **Delete account**, type your email to confirm, and click **Delete account and cloud data**.
android: Go to **More** → **Account & sync**, enter your **Email address** and tap **Send sign-in code**. Type the **Code from the email** and tap **Sign in** (**New code** sends another). Sign out with **Sign out on this device**. To delete your account, tap **Delete account and cloud data**, type your email to confirm and tap **Delete account**.
ios: Go to **More** → **Account & sync** (also in the ⋯ menu). Enter your **Email address**, tap **Send sign-in code**, type the **Code from the email** and tap **Sign in** (**Send a new code** sends another). To sign out, tap **Sign out on this device** → **Sign out**; to delete the account, tap **Delete account and cloud data**, type your email and tap **Delete**.

## sync-setup: Turn on Sync
keywords: sync, sync setup, passphrase, unlock, reset sync, encrypted, other devices, share budget
what: Sync keeps a budget the same on every device, end-to-end encrypted with a passphrase only you know.
desktop: Once you're signed in and subscribed, open **Settings & data** → **Account & sync**. Enter a **Sync passphrase**, enter it again in **Type it again**, and click **Turn on sync**. On another computer, enter the same passphrase and click **Unlock sync**. **Sync now** syncs straight away, **Lock on this computer** makes this computer forget the key, and if you forget the passphrase, **Reset passphrase…** → **Reset sync** sets a new one.
android: After signing in, go to **More** → **Account & sync** → **Set up or unlock sync**. Type a passphrase twice in **Create sync passphrase** (or type it in **Unlock encrypted sync**) and tap **Continue**. The same card then has **Sync automatically**, **Sync now** and **Lock sync on this device**. When offered, a budget kept only on this phone can be added with **Budget library** → long-press → **Add to account sync**. If you forget the passphrase, tap **Reset forgotten sync passphrase** → **Reset cloud copies**.
ios: Once signed in, tap **Set up or unlock sync** in **Account & sync**. On your first device, **Create passphrase** asks for a **Sync passphrase** and **Repeat passphrase**, then **Continue**; on another device, **Unlock sync** takes the same passphrase. **Sync automatically** keeps it running, **Sync now** (also in the ⋯ menu) syncs at once, and **Lock sync on this device** locks it. If you forget the passphrase, use **Reset forgotten sync passphrase**, enter a new one and tap **Reset**.

## sync-subscription: Subscribe to Sync
keywords: subscription, subscribe, pay, price, restore purchase, manage subscription, cancel subscription
what: Sync is the one paid part of UrBudget, a monthly subscription.
desktop: In **Settings & data** → **Account & sync**, click **Subscribe**, which opens urbudget.app in your browser. Subscribe there, then come back and click **I've subscribed**. To manage or cancel the plan, click **Manage account**. There's no separate restore button: sign in with the same email and your plan is picked up.
android: While signed in, go to **More** → **Account & sync** → **Sync subscription**, tap **Show price**, then tap the Subscribe button to buy through Google Play. **Restore purchases** links an existing purchase, and **Manage subscription** opens Google Play to change or cancel it.
ios: Go to **More** → **Account & sync** and sign in, then in the **Sync subscription** section tap "Subscribe for <price> a month" and confirm with the App Store; **Restore purchases** brings back one you already have. **Manage subscription** (for App Store subscriptions) opens Apple's sheet for managing or cancelling it. A subscription bought on urbudget.app or Google Play works after signing in with the same email.

## sync-conflicts: Review sync conflicts
keywords: conflict, conflicts, sync conflict, different on devices, choose version
what: When two devices changed the same thing, the app asks which version to keep.
desktop: Go to **Settings & data** → **Account & sync**, or click the sync chip at the top right when it says **Sync needs you**. For a budget marked **Changed in two places**, click **Review**. Pick **This computer** or the other side for each item (or use the **Keep all:** buttons), then click **Keep these and sync**. If no list is offered, choose **Keep this computer's** or **Use the cloud copy**.
android: When the ⋯ menu shows **Sync needs you**, tap it (or go to **More** → **Account & sync**) and find the budget marked **Needs review**. Tap **Review differences**. For each difference choose **This device** or **Cloud** under **Keep** (or choose under **Keep this complete budget** for the whole budget), then tap **Apply choices**.
ios: When a synced budget needs a decision, the ⋯ menu shows a dot and the **Account & sync** row in **More** shows a count. Open **Account & sync**, find the budget marked **Needs review** under **Budgets** and tap **Review differences**. Choose **This device** or **Cloud** for each difference (or which whole copy wins), then tap **Apply**.

## budgets: Budgets: create, open or switch
keywords: budget, budgets, new budget, create budget, open budget, switch budget, library, rename budget, sample
what: The library holds several budgets; each is separate, with its own accounts and categories.
desktop: Click the budget name at the top of the sidebar to switch to another budget. To create one, choose **New budget…**, enter a **Budget name**, pick an option under **Start with**, and click **Create budget**. **Manage budgets…** shows the whole library, where you can **Open** any budget. To rename the open budget, go to **Settings & data** → **General** → **Budget name**.
android: Tap the budget card at the top of **More** (or **Settings & data** → **Budget library**).
- Under **Budgets**, tap another budget to open it (or long-press → **Open**).
- **New budget**: enter **Budget name** and **Currency** (optionally tick **Keep this budget only on this device**) and tap **Save**.
- To rename, tap the budget marked **Open** → **Budget settings** → **Budget name** → **Save**.
ios: Go to **More** and tap the budget at the top (it says "Switch or manage budgets") to open the **Budget library**; tap a budget to switch to it, or long-press → **Open**. **New budget** asks for the **Budget name**, currency and whether to **Start with sample money**, then **Create**. To rename the open budget, edit **Budget name** in **Settings & data**.

## trash: Trash: restore or delete a budget for good
keywords: trash, delete budget, restore budget, remove budget, empty trash, permanently
what: A deleted budget goes to Trash first, where it can be restored; deleting it from Trash is permanent.
desktop: Click the budget name at the top of the sidebar → **Manage budgets…**, click the trash-can button beside a budget, and confirm with **Move to Trash**. Trashed budgets stay in that list marked **In Trash**, with **Restore** (or **Recover as a copy**). The desktop app can't delete a budget permanently or empty the Trash; the Android app can.
android: In **Budget library**, long-press a budget → **Move to Trash** and confirm with **Move to Trash**. In the **Trash** section, long-press a budget → **Restore** (or **Recover as a new budget**). Budgets kept only on this phone can be removed with **Delete permanently** → **Delete permanently**. **Empty Trash** → **Empty Trash** deletes them all.
ios: In the **Budget library**, swipe left on a budget → **Trash** (or long-press → **Move to Trash**) and confirm **Move to Trash**. The budget stays listed with the status **Trash**; long-press it → **Restore** (or **Recover as a new budget**). Deleting a budget permanently and emptying Trash aren't available on iPhone; the Android app can do both.

## backups: Backups
keywords: backup, backups, restore, recover, retention, automatic backup, keep backups
what: The app keeps backups of a budget and can restore one; how many it keeps can be set.
desktop: Go to **Settings & data** → **Backup & export** and click **Back up now**. **Back up when the app closes** makes a backup automatically, and **Backups to keep** sets how many are kept. To restore, click **Restore…** under **Restore from a backup file**, choose a .json backup, and confirm with **Back up & restore**; the current budget is backed up first.
android: Go to **More** → budget card → **Budget library** → **Backups & recovery**:
- **Create portable backup** saves a copy.
- **Keep automatic backups** sets how many are kept (**Keep all** or the newest 5, 10 or 30).
- Each listed backup has **Restore** → **Restore**.
**Settings & data** → **Backups** also has **Export full backup**, **Import full backup** and **Restore previous budget** (the budget from before your last import).
ios: Go to **More** → tap the budget row → **Budget library** → **Backups & recovery**, tap **Back up now**, and set **Backups to keep** (**All of them**, **The newest 5**, **The newest 10** or **The newest 30**). Under "Backups of <budget>", tap **Restore** on a backup and confirm **Restore** (the current budget is backed up first); long-press a portable backup → **Share**. When there are extra copies, tap "Remove N older backups" → **Remove backups**.

## json-export-import: Export or open a whole budget file
keywords: export budget, json, budget file, open file, import budget, move to another device, share file
what: A whole budget can be saved as one JSON file and opened on another device or app.
desktop: Go to **Settings & data** → **Backup & export** and, under **Full backup file**, click **Export** to save the whole budget as a .json file. To bring one in, click **Restore…** under **Restore from a backup file**, pick the .json file, and confirm with **Back up & restore**. This replaces the open budget, after backing it up.
android: Go to **More** → **Settings & data** → **Export full backup** (or **Budget library** → **Export JSON**) and choose where to save the .json file. To import, tap **Import full backup** (or **Import JSON**), pick the file and confirm **Import backup**; it replaces the open budget.
ios: Go to **More** → **Settings & data** → **Backups**. **Export full backup** saves a portable JSON file; **Import full backup** lets you pick one, and **Import backup** replaces the open budget (you can undo it). A .json opened in UrBudget from Files, Mail or AirDrop asks whether to **Add as a new budget** or **Replace this budget**, and the welcome screen has **Import UrBudget backup**.

## fresh-start: Fresh start
keywords: fresh start, start over, new year, reset budget, clear budget
what: Starts a new budget from the current one's accounts, balances and categories, without its old transactions; the old budget stays as it was.
desktop: Click the budget name at the top of the sidebar → **New budget…** and give it a name. Under **Start with**, choose the "Start over from “<your budget>”" option, then click **Create budget**. The new budget keeps your currency, balances, pending and future transactions, categories, targets, payees, rules and schedules, but drops settled history. Bank connections need linking again, and your old budget stays as it is.
android: Go to **More** → budget card → **Budget library** → **Make a fresh start**, set **New budget name** and **Start date**, and tap **Create fresh budget**. Balances, categories and targets carry into the new budget, which opens; the old one stays in the library.
ios: Go to **More** → tap the budget row → **Budget library** → **Make a fresh start**. Enter the **New budget name** and **Start date**, then tap **Create**. Balances, categories and targets carry into a new budget, which opens, and the old budget stays in the library.

## language: Change the language
keywords: language, arabic, english, switch language, change language, translate, right to left
what: UrBudget is in English and Arabic; Arabic lays the app out right to left.
desktop: Go to **Settings & data** → **General** and pick **System**, **English** or **العربية** under **Language**. The app switches straight away.
android: Go to **More** → **Settings & data**, and under **Language** tap **System**, **English** or **العربية**.
ios: Go to **More** → **Settings & data** → **Appearance** → **Language**, which opens UrBudget's page in the iOS Settings app. Choose Language there and pick English or Arabic (System by default); the app then runs in that language, and Arabic lays it out right to left.

## theme: Light or dark appearance
keywords: theme, dark mode, light mode, appearance, colors, night
what: The app can follow the system's light or dark setting or keep one.
desktop: Go to **Settings & data** → **General** → **Theme** and click **Match system**, **Light** or **Dark**.
android: Go to **More** → **Settings & data**, and under **Appearance** tap **Device**, **Light** or **Dark**.
ios: Go to **More** → **Settings & data** → **Appearance** → **Theme** and choose **System**, **Light** or **Dark**.

## formats: Number, currency and date formats
keywords: format, currency, number format, date format, decimal, comma, symbol
what: How amounts and dates are written, such as the currency symbol, separators and date order.
desktop: Go to **Settings & data** → **General**. In the **Budget** section, set **Currency**, **Number format**, **Currency placement** and **Date format**; changing the currency only changes the label, not the amounts. **Weeks start on** sets the first day of the week in calendars.
android: Go to **More** → **Settings & data** → **Name, currency & formats**, which opens **Budget settings**. Set **Currency**, **Number format**, **Currency placement** and **Date format**, then tap **Save**. Changing the currency only relabels amounts; it doesn't convert them.
ios: Go to **More** → **Settings & data**. Under **Budget**, **Currency** changes the currency label (amounts aren't converted). Under **Numbers and dates**, set **Decimal separator**, **Thousands separator**, **Currency placement** and **Date format**; the **Example** row previews the result.

## flag-names: Name the flags
keywords: flag names, flags, rename flag, colour, color
what: The coloured flags on transactions can be given names.
desktop: In any register, click a transaction's flag spot in the Flag column and choose **Name the flags…**. Type a name for each colour and click **Save names**. If there's no Flag column, turn on **Show a Flag column** in **Settings & data** → **Budget & transactions**.
android: Go to **More** → **Settings & data** → **Flag names**. Type names in **Red flag**, **Orange flag** and the others (leave one blank to keep the colour name), then tap **Save**.
ios: Go to **More** → **Settings & data** → **Preferences** → **Flag names**, type a name next to each colour (for example Reimburse or Tax), and tap **Save**. The names then appear in every **Flag** menu.

## assistant-setup: Set up the assistant
keywords: assistant, ai, chatgpt, openai, anthropic, claude, api key, sign in chatgpt, model
what: The assistant uses your ChatGPT plan (sign in with ChatGPT) or your own OpenAI or Anthropic API key.
desktop: Go to **Settings & data** → **Assistant** and choose a **Service**. For **ChatGPT plan (sign in)**, click **Continue with ChatGPT** and sign in in your browser. For **OpenAI API key** or **Anthropic API key (Claude)**, paste your key in **API key** and click **Save key** (**Get a key** opens the provider's page). You can then pick a **Model**; the assistant panel's own **Continue with ChatGPT** and **Use an API key instead** buttons do the same thing.
android: Open the assistant with the chat-bubble icon at the top of any tab (or **More** → **Assistant**). Tap **Set up assistant** or the gear icon to open **Assistant settings**. Under **Provider**, either:
- choose **ChatGPT plan · browser sign-in** and tap **Continue with ChatGPT**, or
- choose **OpenAI API key** / **Anthropic API key**, paste the key into **API key** and tap **Verify and save key**.
Then pick a **Model** and tap **Done**. **Sign out** or **Remove saved key** disconnects it.
ios: Go to **More** → **Assistant** → **Set up assistant** (or tap the assistant's ⋯ button → **Assistant settings**). Under **Provider**, choose **ChatGPT plan · browser sign-in** and tap **Continue with ChatGPT** to sign in. Or choose **OpenAI API key** / **Anthropic API key**, paste the key into **API key** and tap **Verify and save key** (**Get an API key** opens the provider's page); pick a **Model** if you want one other than the default.

## assistant-history: Assistant conversations
keywords: history, conversation, conversations, new conversation, delete conversation, chat history
what: Conversations are kept in the budget's folder when history is on, and can be reopened or deleted.
desktop: Open the assistant with **Assistant** at the top right, or Ctrl+J / ⌘J. Click the clock button (Conversations) to reopen an earlier conversation or **Delete this conversation**, and the pencil button to start a new one. **Settings & data** → **Assistant** has **Keep conversations**, and **Delete all conversations** → **Delete all**.
android: In the assistant panel, tap the history (clock) icon to open **Conversations**. Tap a conversation to reopen it, tap its trash icon → **Delete** to remove it, or tap **Export current conversation**. The speech-bubble-plus icon starts a new conversation. In **Assistant settings**, **Keep conversation history** turns saving on or off, and **Delete all conversations and attachments** → **Delete all** clears everything.
ios: In the **Assistant**, tap the clock button at the top right to open **Conversations**. Tap one to reopen it, or swipe left → **Delete** (or long-press → **Delete conversation**) and confirm **Delete**; the compose button next to the clock starts a new conversation. **Assistant settings** has the **Keep conversation history** switch and **Delete all conversations and attachments** → **Delete all**.

## undo: Undo and redo
keywords: undo, redo, mistake, revert, go back, take back
what: Most changes can be undone and redone.
desktop: Press Ctrl+Z / ⌘Z to undo and Ctrl+Shift+Z / ⇧⌘Z to redo (Ctrl+Y also redoes on Windows); these shortcuts don't work while you're typing in a field. You can also use the curved-arrow undo and redo buttons at the top right of the page. Many changes also show a message with an **Undo** button.
android: Tap the ⋯ button at the top right of any tab → **Undo** or **Redo**. Many changes also show a snackbar with **Undo** right after, and applied assistant proposals have **Undo this proposal**.
ios: Tap the ⋯ menu (top right on **Budget**, **Transactions**, **Accounts** and **Reports**) → **Undo** or **Redo**. Most changes also show a message with **Undo**, and shaking the iPhone undoes the last change. **Settings & data** → **History** has **Undo last change** and **Redo** too.
