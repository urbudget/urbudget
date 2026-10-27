//! The bank files in `shared/fixtures/bank-files`, and what each holds, for the tests that read them.
#![allow(dead_code)]
use urbudget_core::bank_csv;
use urbudget_core::domain::{Account, AccountKind, Budget, Category};

/// A budget in a currency, with a cash account ("main") and a credit card ("card").
pub fn budget(currency: &str) -> Budget {
    let mut b = Budget { currency: currency.into(), ..Budget::default() };
    for (id, kind) in [("main", AccountKind::Cash), ("card", AccountKind::Credit)] {
        b.accounts.push(Account { extra: Default::default(), id: id.into(), name: id.into(), kind, opening_balance: 0, archived: false });
    }
    b.categories.push(Category {
        extra: serde_json::json!({"paymentAccountId": "card"}).as_object().unwrap().clone(),
        id: "card-payment".into(),
        name: "Card".into(),
        group: "Credit Card Payments".into(),
        target: 0,
    });
    b
}

/// File, currency, account, rows, their total in cents, and a payee one of them should have.
pub const BANKS: &[(&str, &str, &str, usize, i64, &str)] = &[
    // Debit and Credit columns, a constant product column beside the payee's, dd.mm.yy dates.
    ("ch-ubs-classic", "CHF", "main", 6, 455_485, "Coop-1234 Zürich"),
    // "Gutschrift/Lastschrift in CHF", and a pending ("Vormerkung") row left out.
    ("ch-postfinance", "CHF", "main", 6, 455_485, "E-FINANCE ELEKTRIZITÄTSWERK BEISPIELSTADT RECHNUNG 4711"),
    // Two accounts in one file: only the first one's, and a pending ("offen") row left out.
    ("de-comdirect", "EUR", "main", 6, 182_601, "Auftraggeber: Stadtwerke Beispielstadt Buchungstext: Abschlag Strom Oktober Vertragskonto 4711 Ref. 4711-2026-10"),
    // An empty Betrag column, with the money in Soll and Haben.
    ("de-deutsche-bank", "EUR", "main", 6, 182_601, "Stadtwerke Beispielstadt"),
    // A column for each party, one of them the account holder; a pending row left out.
    ("de-dkb-2023", "EUR", "main", 6, 182_601, "Stadtwerke Beispielstadt"),
    // "Umsatz vorgemerkt" left out.
    ("de-sparkasse-camt", "EUR", "main", 6, 182_601, "Stadtwerke Beispielstadt"),
    // Unsigned amounts with S/H under a blank header, balances as rows, an account summary above.
    ("de-volksbank-legacy", "EUR", "main", 6, 182_601, "Stadtwerke Beispielstadt"),
    // A pending ("Venter") row left out.
    ("dk-danske", "DKK", "main", 6, 887_650, "Husleje oktober Boligselskabet Eksempel"),
    // A suggested label beside the bank's.
    ("fr-boursorama", "EUR", "main", 7, -3_452, "Boulangerie Exemple"),
    // No header row at all.
    ("fr-bnpparibas", "EUR", "main", 7, 166_504, "DU 141026 BOULANGERIE EXEMPLE CARTE 4974XXXXXXXX1234"),
    // "Date d'opération" before the value date.
    ("fr-cic", "EUR", "main", 7, 166_504, "VIR EMPLOYEUR EXEMPLE SAS SALAIRE SEPTEMBRE 2026"),
    // Debit Amount and Credit Amount, and a trailing separator.
    ("gb-lloyds", "GBP", "main", 6, 189_566, "THAMES WATER"),
    // Payer and payee names and a merchant, read before the description.
    ("gb-wise-statement", "GBP", "main", 5, -22_168, "Café Lumière"),
    // The amount under "Euro", and a separator inside a description without quotes.
    ("it-unicredit", "EUR", "main", 6, 166_504, "BONIFICO A VOSTRO FAVORE BONIFICO SEPA DA LUCA ESEMPIO PER QUOTA ATTIVITÀ; GRAZIE TRN 0000000003"),
    // Revolut: times after the dates, a fee, a pending row left out.
    ("lt-revolut", "EUR", "main", 5, 32_015, "Corner Café"),
    // D/K beside unsigned sums, opening and closing balances and turnover as rows, Lithuanian names.
    ("lt-swedbank", "EUR", "main", 6, 151_841, "MAXIMA LT, X-123"),
    // 37 lines above the header, a closing balance under it, the shop only in the title.
    ("pl-mbank-legacy", "PLN", "main", 6, 393_512, "ŻABKA Z1234 K.1 /WARSZAWA DATA TRANSAKCJI: 2026-10-02"),
    // A pending ("Reserverat") row where the date goes.
    ("se-nordea", "SEK", "main", 6, 1_656_385, "Bostadsbolaget Exempel AB"),
    // A card's purchases written as positive numbers, in the card's account.
    ("us-amex", "USD", "card", 6, -64_718, "TST* CORNER CAFE SEATTLE WA"),
    // A summary above the header, and a "Beginning balance" row without an amount.
    ("us-bofa-checking", "USD", "main", 6, 136_148, "CORNER CAFE 09/17 PURCHASE SEATTLE WA"),
    // Gross less the fee, holds and conversions' other currency left out, a top-up named by its kind.
    ("us-paypal-activity", "USD", "main", 6, 816, "General Card Deposit"),
    // Chequing and Visa in one file: the first account's, the payee from Description 2.
    ("ca-rbc", "CAD", "main", 5, 178_860, "HYDRO EXAMPLE"),
    // No header row: withdrawals and deposits in columns of their own, then the balance.
    ("ca-td", "CAD", "main", 7, 175_133, "HYDRO EXAMPLE BPY"),
    // A card's file without a header row: purchases, then payments, then the card number.
    ("ca-cibc", "CAD", "card", 5, 43_689, "GROCERY MART #12 TORONTO, ON"),
    // Unsigned amounts with Debit/Credit beside them, the payee in Sub-description.
    ("ca-scotiabank", "CAD", "main", 6, 185_838, "HYDRO EXAMPLE"),
    // Plain text in columns lined up with spaces.
    ("mx-bbva.txt", "MXN", "main", 7, 1_285_544, "SPEI RECIBIDO/0012345 JANE EXAMPLE"),
    // No header row, and a signed running balance beside the signed amount.
    ("au-commbank", "AUD", "main", 8, 3_983, "Direct Credit 401234 ACME PTY LTD SALARY 2809"),
    // A refused ("Geweigerd") payment left out; the other party's name as the payee.
    ("be-bnpparibasfortis", "EUR", "main", 5, 212_511, "SUPERMARKT VOORBEELD"),
    // GBK, 23 lines of notes above the header, 收/支 saying which way unsigned amounts went, and
    // moves between the holder's own balances (不计收支), closed orders and held ones left out.
    ("cn-alipay", "CNY", "main", 3, 6_170, "示例餐厅"),
    ("cn-wechatpay", "CNY", "main", 6, -24_750, "示例便利店"),
    // Hebrew, purchases as positive numbers in a card's file, merchants behind a right-to-left mark.
    ("il-isracard", "ILS", "card", 7, -120_013, "סופר לדוגמה"),
    // Shift_JIS (CP932): money out and money in in columns of their own, and one signed column.
    ("jp-mufg", "JPY", "main", 7, 15_336_000, "サンプルデンキ"),
    ("jp-rakuten", "JPY", "main", 6, 18_411_500, "振込手数料"),
    // Windows-1251, a FAILED payment left out, the account's own currency over the purchase's.
    ("ru-tbank", "RUB", "main", 6, 5_862_675, "Супермаркет Пример"),
    // A commission column taken off, and dashes for empty cells.
    ("ua-monobank", "UAH", "main", 6, 4_004_580, "Залізничні квитки"),
    // From and To columns, the holder in one of them; a pending payment left out.
    ("us-venmo", "USD", "main", 5, -89_325, "Alex Placeholder"),
    // A currency conversion charge taken off the purchase.
    ("nz-anz", "NZD", "main", 7, 230_695, "EXAMPLE STORE LONDON"),
    // Notes in an empty first column, "Withdrawal Amount (INR )", a B/F row of zeros.
    ("in-icici", "INR", "main", 7, 6_577_850, "BIL/ONL/000123456789/EXAMPLE POWER/1234567890"),
    // An Excel workbook whose movements are on its second sheet, with real dates and numbers.
    ("es-bbva.xlsx", "EUR", "main", 6, 173_745, "Mercadona Valencia"),
    // WeChat Pay's bill as it's sent now, an .xlsx.
    ("cn-wechatpay.xlsx", "CNY", "main", 6, -24_750, "示例便利店"),
];

/// File, the format its extension picks, the date order, currency, rows, their total in cents
/// (what the balances the file prints say), and a payee one of them should have.
pub const STATEMENTS: &[(&str, &str, &str, &str, usize, i64, &str)] = &[
    // A batch entry split into its two payments; a pending entry left out.
    ("de-sparkasse.camt053.xml", "camt053", "ymd", "EUR", 5, 221_235, "Sportverein Beispiel e.V."),
    // A statement a day, and the payee's name continued from ?32 to ?33 on the next line.
    ("de-postbank.mt940.sta", "mt940", "ymd", "EUR", 4, 234_936, "Stadtwerke Beispielstadt"),
    // ING's /CNTP/ and /NAME/ keys, a second line under :61:, and the day's :86: summary after it.
    ("nl-ing.mt940.sta", "mt940", "ymd", "EUR", 3, 236_235, "Energie Voorbeeld BV"),
    // A zero-amount verification left out.
    ("us-chase.qfx", "qfx", "ymd", "USD", 3, 133_433, "STARBUCKS STORE 12345"),
    // Quicken's every-account export: the first account's, its payee from the memo where it has none.
    ("us-quicken-all-accounts.qif", "qif", "mdy", "USD", 3, -127_928, "Interest earned"),
    // A name in CDATA, a PAYEE aggregate, and a correction deleting an earlier posting.
    ("ofx2-xml.ofx", "ofx", "ymd", "USD", 6, 126_419, "Boulangerie Éclair & Café"),
    // An intraday account report (camt.052), its pending entries left out.
    ("camt052-intraday.xml", "camt053", "ymd", "EUR", 3, -25_840, "Erika Musterfrau"),
    // A returned direct debit, which names its creditor still.
    ("camt053-pending.xml", "camt053", "ymd", "EUR", 4, 255_000, "ACME Verzekeringen N.V."),
    // An empty record straight after the header, and ISO dates.
    ("qif-iso-dates.qif", "qif", "ymd", "USD", 6, 197_153, "ACME CORP SALARY"),
    // Two days' CAMT.053 files in one ZIP.
    ("de-sparkasse-camt-days.zip", "camt053", "ymd", "EUR", 10, 442_470, "Sportverein Beispiel e.V."),
];

pub fn text(file: &str) -> String {
    let path = format!("{}/../../shared/fixtures/bank-files/{file}", env!("CARGO_MANIFEST_DIR"));
    bank_csv::read_statement(&std::fs::read(&path).unwrap_or_else(|e| panic!("{path}: {e}"))).unwrap().text
}
pub fn csv_name(file: &str) -> String {
    if file.contains('.') { file.to_string() } else { format!("{file}.csv") }
}
