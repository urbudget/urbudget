# Google Play listing

Prepared 5 November 2026 for Android 1.0.0, package `app.urbudget`. It replaces the October draft,
which still sold a trial and a lifetime license.

Google verified the developer's identity on 4 November 2026. The app was created in Play Console on
5 November 2026 (app ID 4974677345566386693, free, category Finance) with this listing. On
12 November 2026 the first bundle, 1.0.0 (version code 1000000, signed on GitHub from `2ac1634` in
[Android release run 43207103396](https://github.com/urbudget/urbudget/actions/runs/43207103396)),
was uploaded to the closed test as a draft release. Nothing has been sent to Google for review.

| Play Console task | State on 5 November 2026 |
| --- | --- |
| Privacy policy, Ads, Advertising ID, Government apps, Health, Financial features | Saved |
| App category and contact details | Saved |
| Store listing | Saved, with no AI label on the assets |
| Content rating | Saved after agreeing to IARC's terms: 3+ in most regions, 14+ in Brazil |
| Data safety | Saved, with all 11 data types answered |
| Sign in details | Saved by the owner, with the sharing switch for Google's test devices left on |
| Target audience | Saved: 18 and over |
| Closed test | Closed testing - Alpha, set up on 12 November 2026: all 178 countries, the "UrBudget Android testers" email list, feedback to `support@urbudget.app`, and release 1.0.1 (version code 1000001, from [#26](https://github.com/urbudget/urbudget/pull/26)), which superseded the first draft, 1.0.0. No errors; Play warns that native debug symbols weren't uploaded. Not yet sent for review |
| SMS declaration | Saved on 12 November 2026: SMS-based money management, the reviewer steps (shortened to Play's 500 characters from [the steps below](#sms-permission-declaration)), and the [walkthrough video](#the-videos) |
| Foreground service | Saved on 12 November 2026: Network processing, Other, with the [service video](#the-videos). The draft release has no errors left, only Play's warning that native debug symbols weren't uploaded. It's saved in Publishing overview, which offers to submit 15 changes for review. Nothing has been submitted |
| Sign in details | Ready on 12 November 2026. "Review account (Google Play)" is `play-review@urbudget.app`, Google Play's own review account with complimentary Sync ([#27](https://github.com/urbudget/urbudget/pull/27)); its code is in Password and its sync passphrase in the instructions. "Assistant for Bank messages" carries a capped OpenAI key at the end of its instructions, since Password takes only 100 characters. The App Store keeps `review@urbudget.app` |

Managed publishing is off, so anything Google approves publishes straight away. Turn it on under
Publishing overview before sending a release if launch day should be a manual choice.

The content rating answers Yes to online content, because Google lists generated AI content as an
example, and Yes to digital purchases, for Sync. Everything else is No.

Play Protect's automatic protection was left on: an installer check is added to builds Play signs.
Builds sideloaded from this repository aren't signed by Play.

## What stops this going live

In the order to deal with them:

1. **Ask Google whether a Personal account can publish this app.** Google requires an Organization
   account for financial products and services, defined as those "related to the management or
   investment of money and cryptocurrencies, including personalized advice". Its examples are
   banking, loans, trading and crypto, but the list is "including but not limited to" those. It
   never names budgeting. Ask through Play Console help before starting the 14-day test, and mention
   the assistant's proposed budget changes. Changing to an Organization account needs a D-U-N-S
   number and can't be undone. The closed-test rule below is written for Personal accounts, but no
   Google page says converting lifts it, so ask that too.
2. **Ask OpenAI whether a Play build that sells Sync can offer ChatGPT plan usage.** Its guide
   describes this route for open-source tools and personal projects that run locally, and sends paid
   apps to a waitlist. UrBudget is open source, but this build sells a subscription. If it isn't
   answered, take ChatGPT out of the description and the build.
3. **Fix the website.**
   - The privacy policy never mentions SMS. It says Android backup may copy budgets when that's
     turned off, and that you choose a device name when Android sends a fixed "Android device"
     (`privacy/app/index.html` lines 36, 53 and 115). Google requires the policy to cover this.
   - The home page says "There's no phone or web version". `vs/quicken`, `vs/everydollar`, `ynab`,
     `ynab/ynab4`, `tools/envelope-budget-template` and `llms.txt` say much the same.
   - The terms and refund pages don't mention Google Play.
4. **Check the disclosure in the app.** It appears right before the SMS permission, so Google
   reviews it closely. The errors this list found were fixed on 9 November 2026, in English and
   Arabic: it says texts go to the assistant's provider (when the page opens, as they arrive if
   that's on, and when asked about), that text-only MMS is read and RCS isn't, the sending notice
   names transfer routes with amounts and code texts with the code hidden, the connect notice
   points to More → Assistant, and Not now sits beside Allow. Read it once more against the
   [SMS declaration](#sms-permission-declaration) and the privacy policy before sending a release.
5. **File the SMS declaration below.** An active bundle with `READ_SMS`, on any track including
   internal, blocks every change to the app, including the store listing, until you create a release
   that includes the declaration or removes the permission. Review can take up to several weeks, and
   the release stays pending until then.
   The bundle also declares a foreground service since 11 November 2026
   (`FOREGROUND_SERVICE_DATA_SYNC`, for `SmsReadingService`), which Play asks about under App
   content, Foreground service permissions. Choose Data sync, and say: "When the person taps to
   have their bank messages read, UrBudget sends the messages from the senders they chose to the
   AI provider they connected, a request at a time, which can take several minutes. The service
   keeps that going if they leave the app, shows a notification that opens Bank messages, and
   ends when the reading does. It never starts by itself." Play wants a short video of it: start
   a reading, leave the app, and show the notification. Not yet tried on a device.
6. **Give the reviewer what bank texts need:** a working assistant and a test text on the phone. See
   [App access](#app-access).
7. **Run the closed test.** At least 12 testers, opted in for 14 days in a row, before you can apply
   for production. Google's review of that application usually takes 7 days or less. People join
   from the home page's Android section; the admin page's Health tab copies their addresses for the
   "UrBudget Android testers" list. Once the test is published, each opts in at
   `https://play.google.com/apps/testing/app.urbudget` with the Google account they gave.
8. **Decide on trader status.** See [Other forms](#other-forms).

Also to do before release, from [MOBILE.md](../MOBILE.md#android-launch-configuration): create the
`urbudget_sync` subscription, connect purchase checking, and test a real signed Play build.

## Words to avoid

- **"Give every dollar a job".** YNAB's terms list this phrase as its trademark and bar using it
  without written permission. It's been kept out of this listing. It's also on the App Store's first
  screenshot, in the October Microsoft Store draft, and on six pages of the website. Whether the mark
  is registered wasn't checked.
- **YNAB's personal access token.** YNAB says these tokens are for the account owner and shouldn't
  be shared, and that OAuth is the only option for apps other people use. The listing only mentions
  export files. The app still offers the token, so ask YNAB or move to OAuth before a Play release.

## The videos

Recorded on 12 November 2026 on a Galaxy Z Fold (cover screen) with the 1.0.0 bundle uploaded to the
closed test, installed from it with bundletool and the debug key. The sample budget, assistant
connected with a ChatGPT plan, and made-up bank texts the phone sent to its own number: Android
hides texts that `adb` writes into the SMS store from every app, so they have to arrive as real SMS.
The phone number, the phone's real senders, its home screen and other notifications are blurred, and
a text-recognition pass over every 0.1 s of both videos found none of them readable. Both are in the
developer account's Google Drive, shared with anyone who has the link:

- [SMS permission walkthrough](https://drive.google.com/file/d/1t096WVFuideQfhXCD2Ab_PhkEssEBziy/view?usp=sharing),
  2:03: opening the app, the disclosure, declining and carrying on, the disclosure again, allowing,
  Android's prompt, choosing the sender, the sending notice naming ChatGPT, the review and adding.
- [Foreground service](https://drive.google.com/file/d/1qtx2mzY21KDw_kBZgk2gUqFogeQE6NTs/view?usp=sharing),
  0:27: starting a reading, leaving the app, the notification, and it going when the reading ends.

What the recording found, both fixed in 1.0.1 ([#26](https://github.com/urbudget/urbudget/pull/26)):

- **The reading's notification needed notification permission, which a fresh install didn't ask
  for.** `SmsReadingService` ran without it, but its notification wasn't shown, because the app asked for
  `POST_NOTIFICATIONS` only when "Read new texts as they arrive" was turned on. The service video was
  recorded after allowing notifications by hand. 1.0.1 asks once as the first reading starts, and was
  tried on the Fold with a fresh install: the prompt came, the notification showed, and it went when the
  reading ended.
- **On the Fold's cover screen, the disclosure's Allow SMS access button started under the floating tab
  bar**, with Not now half covered, until the page was scrolled. In 1.0.1 the tab bar steps aside while
  the disclosure shows, and the buttons stay after the whole disclosure.
- A reading of a few texts in a format the assistant already learned ends in seconds, so the
  notification is gone almost at once; new formats keep it up longer.

## Store listing

| Field | Limit | Value |
| --- | --- | --- |
| App name | 30 | `UrBudget: Envelope Planner` |
| Default language | | English (United States) |
| App or game | | App |
| Category | | Finance |
| Free or paid | | Free. Sync is an in-app subscription, so the app stays Free |
| Contact email | | `support@urbudget.app` |
| Website | | `https://urbudget.app` |
| Privacy policy | | `https://urbudget.app/privacy/app/` |
| Account deletion | | `https://urbudget.app/delete-account/` |

The name matches the App Store's. `UrBudget` alone also works and matches the phone's label.

**Short description** (80 characters):

```text
Envelope budgeting that turns your bank's texts into transactions you review
```

**Full description** (4,000 characters). It leads with bank texts on purpose: Google requires a
feature that uses SMS to be core to the app and prominently promoted in the description.

```text
UrBudget is an envelope budget for your phone. You give all your money a category, spend from those categories, and move money when plans change. It works offline, has no ads, and budgeting needs no account.

Turn your bank's texts into transactions
If your bank texts you when money moves, UrBudget can turn those texts into transactions, so your budget keeps up without linking your bank or typing each purchase.
• Choose which senders to read and how far back to start
• The assistant you connect reads the texts, and the phone learns each sender's format as it goes
• Cards and accounts are matched to your budget, and one choice answers every text that names that card
• Review every transaction before it's added, then undo the whole batch in one step if you need to
• Optionally read new texts as they arrive, with a notification

Bank texts are optional, and need an AI assistant you pay for and connect: a ChatGPT Plus or Pro plan, or your own OpenAI or Anthropic API key, billed by that provider. Android's permission covers all your text messages, so UrBudget keeps only texts from the senders you pick. Those texts, along with your account names, categories, payees and recent transfers, go from your phone straight to that provider, and never to UrBudget's server. You're asked once for each assistant, then texts are sent without asking until you stop. Results depend on your bank's texts, so check them.

Budget the money you have
Start with what's in your accounts and give all of it a category: rent, groceries, the car insurance that's due in March. Spending comes out of its category. If you overspend, cover it from another category, and whatever is left carries into next month.
• Targets for weekly, monthly and yearly costs, or for an amount by a date
• Card spending from funded categories moves into the card's payment category, ready for the bill
• Quick assign fills underfunded categories or repeats last month, and shows the changes first

Track every transaction
• A keypad that does sums, and payees that remember their category
• Splits, transfers, flags and search
• Approve or clear many rows at once, with undo and redo
• Scheduled pay, rent and subscriptions are added when you open the app once they're due, for you to approve

Bring in your history
• Bank files: CSV, Excel, OFX, QFX, QIF, CAMT.053 and MT940, each reviewed before anything is added, with duplicates flagged
• Supported US and Canadian banks through SimpleFIN, which charges its own fee
• Moving from YNAB: bring your budget with its export files. UrBudget isn't affiliated with YNAB

Twelve reports and a loan planner
Spending by category and payee, income vs. expense, net worth, a balance forecast, cash age and more. The loan planner shows the payment schedule, and the time and interest an extra payment saves.

An assistant that knows your budget (optional)
Ask about your budget and get proposed changes that you apply yourself. The assistant uses generative AI through your own ChatGPT Plus or Pro plan or API key, and sends your messages, any files you attach and the budget details it looks up to that provider. AI can be wrong, so check its work, and use Report under any reply to tell us about a problem. Its replies aren't financial advice. Only bank texts and PDF statements need it.

Sync between your devices (optional)
Keep a budget the same on your phone and your computer. Sync is encrypted with your own passphrase before it leaves your device, so the server holds data it can't read. Sync needs an UrBudget account and a monthly subscription that renews until you cancel it in Google Play. The price is shown in Google Play before you buy. It's the only thing UrBudget charges for.

Also in UrBudget
• English and Arabic, with full right-to-left support
• One currency per budget, with your choice of number and date formats
• Several budgets, and full backups you can save and restore

UrBudget can't move your money, and it never asks for your bank login.
```

**Release notes** (500 characters):

```text
The first release of UrBudget on Google Play.
```

## Graphics

All in [graphics/](graphics/). Upload the screenshots in this order.

| File | Use | Size |
| --- | --- | --- |
| `icon-512.png` | App icon | 512×512, 32-bit PNG. Square corners: Play applies its own mask |
| `feature-graphic-1024x500.png` | Feature graphic | 1024×500, 24-bit PNG |
| `1-budget.png` | Phone screenshot | 1440×2560, 24-bit PNG |
| `2-bank-messages.png` | Phone screenshot | 1440×2560, 24-bit PNG |
| `3-track-spending.png` | Phone screenshot | 1440×2560, 24-bit PNG |
| `4-reports.png` | Phone screenshot | 1440×2560, 24-bit PNG |
| `5-scheduled.png` | Phone screenshot | 1440×2560, 24-bit PNG |
| `6-accounts-dark.png` | Phone screenshot | 1440×2560, 24-bit PNG |

Alt text (140 characters or fewer, recommended):

| Image | Alt text |
| --- | --- |
| 1 | Budget for the month, with $8,400 unassigned above categories showing what's assigned and available. |
| 2 | Review of bank texts turned into transactions, each ticked, with balances before and after adding them. |
| 3 | New transaction for $42.18 at Neighborhood Market, with its category filled in as Groceries. |
| 4 | Spending report for October with a bar of top categories, and income compared with spending by month. |
| 5 | Scheduled bills and income, with totals going out and coming in over the next 30 days. |
| 6 | Accounts in the dark theme, grouped into cash, credit and tracking, with net worth at the top. |
| Feature graphic | Budget screen beside the words: Budget the money you have. |

How they were made:

- Each screen is an unedited emulator capture of the debug build of the code merged in
  [#15](https://github.com/urbudget/urbudget/pull/15), cropped only to remove the status bar. The
  first screenshot also enlarges its Unassigned card from the same capture.
- The budget is the synthetic showcase budget, with real brands swapped for generic names, because
  Play doesn't allow other companies' marks without permission.
- The bank texts in screenshot 2 are made up, and their reading was seeded for the capture. No
  assistant ran.
- Tablet screenshots are optional. Since November 2026 the app has a large-screen layout (a tab
  rail, lists beside what's opened from them, sheets and the assistant at the side, and it keeps
  clear of a Fold's hinge), so 7- and 10-inch captures can come from a tablet or Pixel Fold
  emulator; none are taken yet.

Recapture if the screens change.

The store listing's Review step asks whether to label assets as created or edited using AI, and
Google's rule covers images and text. The owner chose not to label them: the screens are real
captures, though an AI wrote the captions and description and laid out the graphics.

## Data safety

Based on an audit of the code at `36bbe39`. Sync's budget contents are end-to-end encrypted, so they
don't count as collected. Account, purchase and assistant data do.

**Your decision: is data sent to OpenAI and Anthropic "shared"?** They work under the user's own
account, so they aren't your service providers. "No" would rest on Google's exception for sending
with prominent consent. **Use Yes.** Bank texts are the part Google will look at hardest, Yes matches
what the description says, and it only adds a line to the listing. The table assumes Yes.

Form-level answers:

| Question | Answer |
| --- | --- |
| Does the app collect or share any required user data types? | Yes |
| Is all data encrypted in transit? | Yes |
| Can users create an account? | Yes, by email with a one-time code |
| Can users ask for their data to be deleted? | Yes, in the app and at the deletion URL |
| Ephemeral processing | No: the providers keep data for a time |

Every collected type is optional. Each one's purpose is App functionality, plus any listed here.

| Data type | Collected | Shared | Also for | Why |
| --- | --- | --- | --- | --- |
| Email address | Yes | No | Account management | Sign-in. Resend and Cloudflare are service providers |
| User IDs | Yes | No | Account management | Account ID, and its hash at purchase |
| User payment info | Yes | Yes | | Texts and account names can carry a card's last digits. No full numbers are asked for |
| Purchase history | Yes | Yes | Account management | Transactions to the assistant, Sync's state to UrBudget |
| Other financial info | Yes | Yes | | Balances, budgets, income and loans to the assistant |
| SMS or MMS | Yes | Yes | | Bank texts to the assistant, never to UrBudget |
| Other in-app messages | Yes | Yes | | Assistant chats, and reports to UrBudget |
| Photos | Yes | Yes | | Images attached to a message |
| Files and docs | Yes | Yes | | PDFs and spreadsheets attached to a message |
| Other user-generated content | Yes | Yes | | Budget, account and payee names, memos, report text |
| Device or other IDs | Yes | Yes | | Sync's random installation ID to UrBudget, and a random ID to OpenAI at ChatGPT sign-in |
| Everything else | No | No | | Includes name, address, phone, credit score, location, contacts, calendar, browsing and crash logs |

Check before submitting:

- **Diagnostics:** UrBudget's code sends none, but Google's Billing library bundles a log-upload
  component, and what it sends wasn't found. Check Billing's entry in the
  [Google Play SDK Index](https://developer.android.com/distribute/sdk-index).
- **App interactions:** each assistant message names the page, month, account and category on
  screen, and what happened to earlier proposals. If that counts, answer Collected Yes, Shared Yes.
- **Reports** partly serve to investigate harmful output, so Fraud prevention, security and
  compliance may also fit Other in-app messages.

## SMS permission declaration

In Play Console, under App content, when the first bundle is uploaded. The exception is **SMS-based
money management**, which lists budget tracking as its example.

**Core functionality to tick:** SMS-based money management.

**Why the app needs these permissions:**

```text
UrBudget is a personal budgeting app. Tracking spending is its core purpose, and Bank messages turns the transaction texts a person's bank sends into budget transactions.

READ_SMS lists who has texted the phone, with a count for each sender and a short preview for short codes, so the person can choose their bank. That list isn't kept or sent. It then collects SMS and text-only MMS from the chosen senders, from a date they pick. When the person asks the in-app assistant about bank texts, it also searches the chosen senders' texts on the phone by date, amount or words, to show what happened to a transaction; no other sender's texts are searched.

RECEIVE_SMS collects new texts as they arrive, including while the app is closed. It sees every incoming text and keeps only those from chosen senders, on the phone. They go to the AI provider when the person opens Bank messages, on arrival if they turn on Read new texts as they arrive, or when the person asks the assistant about bank texts and it finds them.

There's no equivalent for many people: banks in many countries offer no statement download or data connection, and text messages are the only record that arrives as money moves. Android offers no other way to read these texts, and the app doesn't request default SMS handler status or send or change messages.

Texts from chosen senders are sent to the AI provider the person connected, only after a dialog naming that provider, so that it can read each transaction. They never reach UrBudget's servers. Nothing is added to a budget until the person reviews it, and they can revoke sending, or stop and clear a budget's text data, on the Bank messages page. The permissions are optional: without them every other feature works.
```

**Instructions for the reviewer:**

```text
1. Budgeting needs no sign-in. On the first screen, tap Explore with sample money.
2. Connect the assistant as App access describes, then close it.
3. From another phone, send this device: Purchase of 12.50 at City Market with card ending 1234. Balance 840.00.
4. Open More, then Bank messages. Read the notice, tap Allow SMS access, and allow Android's prompt.
5. In Senders, tap Show 1 phone number and tick that number. Under Collect, choose Everything on this phone, then tap Save.
6. Tap Read them with OpenAI, read the sending notice, and tap Send to OpenAI.
7. When reading ends, the review opens. Make any choice it asks for, then tap Add 1 transaction.
To decline, deny Android's prompt: the rest of the app keeps working.
```

Run these on a clean install once the disclosure is fixed, and add the steps the assistant's first
reading needs. If people's numbers are hidden from the picker, the reviewer will need another way to
get a test text.

**The video** Google requires shows, in one take with made-up texts:

1. Opening the app.
2. The way to Bank messages, and the whole disclosure, slowly enough to read.
3. Declining, the app carrying on, then opening Bank messages again so the disclosure comes back.
4. Allowing, then Android's prompt.
5. Choosing senders, and the sending dialog naming the provider.
6. Texts turning into a review, and adding them.

**Where Google might refuse:**

- SMS is opt-in and the app has other imports, so a reviewer could say it isn't core.
- Google bars sharing non-financial or personal SMS history. The sender list can include a person's
  number, and marketing and security texts from a chosen sender do reach the assistant. Consider
  hiding people's numbers from the picker, and filtering more of this on the phone.
- Google's pages don't ban sending texts to an AI provider, but its July 2026 update applies the User
  Data rules to third-party AI integrations and keeps you responsible. SMS data can be shared only to
  provide core features, never for purposes such as improving other services, so check what each
  provider does with it.

## App access

Now called **Sign in details** in Play Console. Choose Yes, part of the app is restricted, and add
this there only. The access code, passphrase and API key never go in the repository. Use Review
account as the name, `review@urbudget.app` as the username and the access code as the password. The
instructions field holds 500 characters:

```text
Budgeting needs no sign-in: tap Explore with sample money. Sync: More, Account & sync, enter the email, tap Send sign-in code. No email comes: type the password above as the code, tap Sign in, then unlock sync with passphrase [ADD PASSPHRASE]. Bank messages: More, Assistant, Set up assistant, Provider OpenAI API key, paste [ADD KEY], Verify and save key, Done. Then follow the SMS declaration's steps.
```

An OpenAI key may not fit in the 97 characters left. If it doesn't, add a second set of sign in
details for the key.

The review account is `review@urbudget.app`, set up for the App Store with complimentary Sync. Its
code is in the private signing folder. Check it still works on Play's reviewer path, and whether App
Review set a passphrase, because Play's reviewer will need the same one.

**Your decision:** the API key has to be a real one. Make a separate key with a low spending limit,
and revoke it after review.

## Other forms

| Form | Answer |
| --- | --- |
| Ads | No ads |
| Financial features | Not settled by Google's pages. Best reading: Other, under Support services, and possibly Financial advice because of the assistant. Ask in the same support request as the Organization question. Don't choose None: Google's definition covers managing money |
| Target audience | 18 and over. The privacy policy says it isn't directed at children under 16 |
| Content rating | Answer the questionnaire as a utility app: no violence, sexual content, gambling, drugs or strong language, and no sharing between users. It does sell digital goods (Sync). Expect Everyone or 3+ |
| Government apps | No |
| Health apps | Not a health app |
| News apps | Not a news app |
| Generative AI | The assistant is user-prompted, and every reply has an in-app Report button |
| Foreground service | WorkManager adds this permission. If Play Console asks, it's for background work the user turned on |
| Countries | Your decision. Bank texts work wherever a bank's texts can be read, but SimpleFIN covers only the US and Canada |

**Trader status, your decision.** Google requires every active account to declare trader status
under EEA and UK law, wherever its apps are distributed, and shows that status in the EEA and UK. If
you don't distribute there, you can declare as a non-trader. If you do, selling Sync points toward
yes, and a trader must also give a terms of service link for each app. A Personal account declared
as a trader shows your phone number and address publicly, and declaring wrongly risks termination.
Existing accounts have until 1 February 2027.
