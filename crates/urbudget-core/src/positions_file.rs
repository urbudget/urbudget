//! What an investment account holds, read from a file a broker gives: the positions it
//! exports as CSV, or an OFX/QFX investment statement. It becomes a snapshot for
//! `investments::record`, the way a bank's file becomes rows for the bank import.
//!
//! A CSV is read by its header row, wherever it is: a column for each holding's name or symbol
//! and one for its value (or its quantity and price). Rows that are totals are passed over and
//! cash is kept apart. A layout this can't read is one for the assistant, which gives the same
//! snapshot through `propose_holdings`.

use crate::bank_csv::{fields, header, plain, separators};
use crate::bank_statements::tag;
use crate::flex::Dec;
use crate::investments::Position;
use crate::t;
use serde::Serialize;

/// What a file says an account holds.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Read {
    /// "csv" or "ofx".
    pub format: String,
    /// The day the figures are from, when the file says; else empty.
    pub as_of: String,
    /// The currency the values are in, when the file says; else empty.
    pub currency: String,
    pub positions: Vec<Position>,
    /// Cash not invested, in the same currency; empty when the file lists none.
    pub cash: String,
    /// Everything it lists added up, cash included.
    pub value: String,
    /// Rows that looked like holdings and had no value this could read.
    pub skipped: usize,
}

fn not_found() -> String {
    t!("UrBudget couldn't find holdings in this file. It looks for a row for each holding, with a name or symbol and a value.")
}

/// A figure as a broker writes it ("$1,234.56", "1.234,56 €", "(12.34)", "1 234,5"), or None
/// for "--", "n/a" and words.
pub fn number(s: &str) -> Option<Dec> {
    let t = s.trim();
    let negative = t.starts_with('(') && t.ends_with(')') || t.starts_with(['-', '−']) || t.ends_with(['-', '−']);
    let digits: String = t.chars().filter(|c| c.is_ascii_digit() || *c == '.' || *c == ',').collect();
    if !digits.bytes().any(|b| b.is_ascii_digit()) {
        return None;
    }
    let plain = match (digits.rfind('.'), digits.rfind(',')) {
        // "1.234,56": the comma marks the cents.
        (Some(dot), Some(comma)) if comma > dot => digits.replace('.', "").replace(',', "."),
        (Some(_), Some(_)) => digits.replace(',', ""),
        // A single comma not followed by three digits is a decimal mark ("12,5"); else grouping.
        (None, Some(comma)) if digits.matches(',').count() == 1 && digits.len() - comma - 1 != 3 => digits.replace(',', "."),
        (None, Some(_)) => digits.replace(',', ""),
        (Some(_), None) if digits.matches('.').count() > 1 => digits.replace('.', ""),
        _ => digits,
    };
    Dec::parse(&plain).map(|d| if negative { d.negated() } else { d })
}

/// The broker's kind of holding, from what a file calls it.
fn kind(text: &str) -> &'static str {
    let t = plain(text);
    let has = |words: &[&str]| words.iter().any(|w| t.contains(w));
    if has(&["etf", "fund", "fonds", "mutual", "trust", "صندوق"]) {
        "FUND"
    } else if has(&["bond", "fixedincome", "treasury", "bill", "note", "anleihe", "obligation", "sukuk", "صكوك", "سند"]) {
        "BOND"
    } else if has(&["option", "warrant"]) {
        "OPT"
    } else if has(&["crypto", "coin"]) {
        "CRYPTO"
    } else if has(&["stock", "equity", "equities", "share", "aktie", "action", "سهم", "اسهم", "أسهم"]) {
        "STK"
    } else {
        ""
    }
}

const SYMBOL: &[&str] = &["symbol", "ticker", "tickersymbol", "isin", "wkn", "cusip", "code", "securitycode", "instrumentcode", "الرمز", "رمز"];
const NAME: &[&str] = &[
    "name", "description", "security", "securityname", "securitydescription", "investment", "investmentname", "holding", "asset", "assetname",
    "instrument", "instrumentname", "product", "company", "fund", "fundname", "bezeichnung", "wertpapier", "libelle", "designation", "nombre",
    "descripcion", "الاسم", "اسم", "الشركة", "الوصف", "البيان",
];
const QUANTITY: &[&str] = &[
    "quantity", "qty", "shares", "units", "sharesunits", "unitsshares", "position", "holdings", "numberofshares", "stuck", "stueck", "anzahl",
    "nominale", "quantite", "cantidad", "الكمية", "كمية", "عدد",
];
const PRICE: &[&str] = &[
    "price", "lastprice", "currentprice", "marketprice", "closingprice", "close", "last", "shareprice", "unitprice", "nav", "priceshare", "kurs",
    "aktuellerkurs", "cours", "precio", "السعر", "سعر", "سعرالسوق",
];
const VALUE: &[&str] = &[
    "value", "marketvalue", "currentvalue", "totalvalue", "mktval", "mktvalue", "positionvalue", "valuation", "worth", "balance", "amount",
    "kurswert", "marktwert", "aktuellerwert", "wert", "valorisation", "montant", "valor", "valordemercado", "القيمة", "قيمة", "القيمةالسوقية",
];
const COST: &[&str] = &[
    "cost", "costbasis", "costbasistotal", "totalcost", "totalcostbasis", "bookvalue", "bookcost", "purchasevalue", "amountinvested", "invested",
    "einstandswert", "kaufwert", "prixderevienttotal", "coste", "التكلفة", "تكلفة", "التكلفةالاجمالية",
];
/// What one share cost: times the quantity, it is the cost.
const UNIT_COST: &[&str] = &[
    "averagecost", "avgcost", "avgprice", "averageprice", "costpershare", "costshare", "unitcost", "purchaseprice", "buyprice", "costprice",
    "averagecostbasis", "einstandskurs", "kaufkurs", "prixderevient", "pru", "متوسطالتكلفة", "سعرالشراء",
];
const CURRENCY: &[&str] = &["currency", "ccy", "cur", "curr", "wahrung", "devise", "moneda", "divisa", "العملة", "عملة"];
const KIND: &[&str] = &["type", "assettype", "assetclass", "securitytype", "instrumenttype", "kind", "category", "art", "النوع", "نوع"];
const TOTALS: &[&str] = &["total", "totals", "accounttotal", "grandtotal", "subtotal", "totalvalue", "summe", "gesamt", "الاجمالي", "الإجمالي", "المجموع"];
const CASH: &[&str] = &[
    "cash", "cashcashinvestments", "cashbalance", "cashandcashequivalents", "cashandsweepvehicle", "uninvestedcash", "freecash", "cashavailable",
    "barmittel", "liquiditat", "especes", "liquidites", "efectivo", "نقد", "النقد", "نقدية",
];

/// Where each thing is among a header row's cells.
#[derive(Default)]
struct Columns {
    symbol: Option<usize>,
    name: Option<usize>,
    quantity: Option<usize>,
    price: Option<usize>,
    value: Option<usize>,
    cost: Option<usize>,
    unit_cost: Option<usize>,
    currency: Option<usize>,
    kind: Option<usize>,
    /// The currency the value column names in brackets ("Market Value (USD)").
    stated: Option<String>,
}
/// The columns a row of cells names, when it is a header: something to call a holding by, and
/// its value or what gives it.
fn columns(cells: &[String]) -> Option<Columns> {
    let mut c = Columns::default();
    for (i, cell) in cells.iter().enumerate() {
        let (name, code) = header(cell);
        let name = name.as_str();
        let slot = if SYMBOL.contains(&name) {
            &mut c.symbol
        } else if NAME.contains(&name) {
            &mut c.name
        } else if QUANTITY.contains(&name) {
            &mut c.quantity
        } else if PRICE.contains(&name) {
            &mut c.price
        } else if VALUE.contains(&name) {
            if c.value.is_none() {
                c.stated = code;
            }
            &mut c.value
        } else if COST.contains(&name) {
            &mut c.cost
        } else if UNIT_COST.contains(&name) {
            &mut c.unit_cost
        } else if CURRENCY.contains(&name) {
            &mut c.currency
        } else if KIND.contains(&name) {
            &mut c.kind
        } else {
            continue;
        };
        // The first column of a name is the one meant: a second "Value" is a day's change or a share.
        slot.get_or_insert(i);
    }
    ((c.symbol.is_some() || c.name.is_some()) && (c.value.is_some() || c.quantity.is_some() && c.price.is_some())).then_some(c)
}

fn csv(content: &str) -> Result<Read, String> {
    let lines: Vec<&str> = content.lines().collect();
    // The header row, wherever a broker's preamble leaves it, and what splits its cells.
    let found = lines.iter().enumerate().take(80).find_map(|(at, line)| separators(line).find_map(|sep| Some((at, sep, columns(&fields(line, sep))?))));
    let (at, sep, c) = found.ok_or_else(not_found)?;
    let mut out = Read { format: "csv".into(), currency: c.stated.clone().unwrap_or_default(), ..Read::default() };
    let (mut cash, mut any_cash) = (Dec::ZERO, false);
    let mut total = Dec::ZERO;
    let mut currencies: Vec<String> = vec![];
    for line in &lines[at + 1..] {
        let cells = fields(line, sep);
        let cell = |i: Option<usize>| i.and_then(|i| cells.get(i)).map(|s| s.trim()).unwrap_or("");
        let (symbol, name) = (cell(c.symbol), cell(c.name));
        let label = if name.is_empty() { symbol } else { name };
        if label.is_empty() || TOTALS.contains(&plain(label).as_str()) || TOTALS.contains(&plain(symbol).as_str()) {
            continue;
        }
        let quantity = number(cell(c.quantity));
        let price = number(cell(c.price));
        let value = number(cell(c.value)).or_else(|| Some(quantity?.times(price?)));
        let is_cash = [label, symbol, cell(c.kind)].iter().any(|t| CASH.contains(&plain(t).as_str()));
        let Some(value) = value else {
            // A footnote under the table has no figures at all; a holding without a value is one to say.
            if quantity.is_some() || price.is_some() {
                out.skipped += 1;
            }
            continue;
        };
        total = total.plus(value);
        if is_cash {
            cash = cash.plus(value);
            any_cash = true;
            continue;
        }
        let currency = cell(c.currency).to_ascii_uppercase();
        if currency.len() == 3 && currency.bytes().all(|b| b.is_ascii_uppercase()) && !currencies.contains(&currency) {
            currencies.push(currency.clone());
        }
        let cost = number(cell(c.cost)).or_else(|| Some(number(cell(c.unit_cost))?.times(quantity?)));
        out.positions.push(Position {
            symbol: symbol.to_string(),
            name: name.to_string(),
            kind: kind(cell(c.kind)).to_string(),
            currency: if currency.len() == 3 { currency } else { String::new() },
            quantity: quantity.map(Dec::text).unwrap_or_default(),
            price: price.map(Dec::text).unwrap_or_default(),
            value: value.text(),
            cost: cost.map(Dec::text).unwrap_or_default(),
        });
    }
    if out.positions.is_empty() && !any_cash {
        return Err(not_found());
    }
    // One currency named for every holding is the file's; several leave it to be said.
    if out.currency.is_empty() && currencies.len() == 1 {
        out.currency = currencies.remove(0);
    }
    if any_cash {
        out.cash = cash.text();
    }
    out.value = total.text();
    Ok(out)
}

/// "20261113120000[-5:EST]" as "2026-11-13".
fn ofx_day(text: &str) -> Option<String> {
    let digits: String = text.chars().take_while(char::is_ascii_digit).collect();
    let day = format!("{}-{}-{}", digits.get(..4)?, digits.get(4..6)?, digits.get(6..8)?);
    crate::domain::valid_date(&day).then_some(day)
}
/// The pieces of `content` that each start right after one `<NAME>` and run to the next.
fn blocks<'a>(content: &'a str, upper: &str, name: &str) -> Vec<&'a str> {
    let needle = format!("<{name}>");
    let starts: Vec<usize> = upper.match_indices(&needle).map(|(i, _)| i + needle.len()).collect();
    starts.iter().enumerate().map(|(n, &from)| &content[from..starts.get(n + 1).map_or(content.len(), |next| next - needle.len())]).collect()
}

fn ofx(content: &str) -> Result<Read, String> {
    let upper = content.to_ascii_uppercase();
    // Each security's name and ticker, by the id its positions name it with.
    let securities: Vec<(String, String, String)> = blocks(content, &upper, "SECINFO")
        .into_iter()
        .filter_map(|s| Some((tag(s, "UNIQUEID")?, tag(s, "SECNAME").unwrap_or_default(), tag(s, "TICKER").unwrap_or_default())))
        .collect();
    let mut out = Read { format: "ofx".into(), currency: tag(content, "CURDEF").unwrap_or_default().to_ascii_uppercase(), ..Read::default() };
    let mut total = Dec::ZERO;
    let mut priced: Vec<String> = vec![];
    for (name, kind) in [("POSSTOCK", "STK"), ("POSMF", "FUND"), ("POSDEBT", "BOND"), ("POSOPT", "OPT"), ("POSOTHER", "")] {
        for block in blocks(content, &upper, name) {
            // A position's figures come before the next position of any kind starts.
            let capitals = block.to_ascii_uppercase();
            let end = ["<POSSTOCK>", "<POSMF>", "<POSDEBT>", "<POSOPT>", "<POSOTHER>", "</INVPOSLIST>"].iter().filter_map(|next| capitals.find(next)).min().unwrap_or(block.len());
            let block = &block[..end];
            let id = tag(block, "UNIQUEID").unwrap_or_default();
            let known = securities.iter().find(|s| s.0 == id);
            let quantity = tag(block, "UNITS").and_then(|t| number(&t));
            let price = tag(block, "UNITPRICE").and_then(|t| number(&t));
            let Some(value) = tag(block, "MKTVAL").and_then(|t| number(&t)).or_else(|| Some(quantity?.times(price?))) else {
                out.skipped += 1;
                continue;
            };
            total = total.plus(value);
            if let Some(day) = tag(block, "DTPRICEASOF").and_then(|t| ofx_day(&t)) {
                priced.push(day);
            }
            out.positions.push(Position {
                symbol: known.map(|s| s.2.clone()).filter(|t| !t.is_empty()).unwrap_or(id),
                name: known.map(|s| s.1.clone()).unwrap_or_default(),
                kind: kind.to_string(),
                currency: String::new(),
                quantity: quantity.map(Dec::text).unwrap_or_default(),
                price: price.map(Dec::text).unwrap_or_default(),
                value: value.text(),
                cost: String::new(),
            });
        }
    }
    let cash = tag(content, "AVAILCASH").and_then(|t| number(&t)).filter(|c| !c.is_zero());
    if out.positions.is_empty() && cash.is_none() {
        return Err(not_found());
    }
    if let Some(cash) = cash {
        total = total.plus(cash);
        out.cash = cash.text();
    }
    out.as_of = blocks(content, &upper, "INVSTMTRS")
        .first()
        .and_then(|statement| tag(statement, "DTASOF"))
        .and_then(|t| ofx_day(&t))
        .or_else(|| priced.into_iter().max())
        .unwrap_or_default();
    out.value = total.text();
    Ok(out)
}

/// What a broker's file says an account holds: an OFX or QFX investment statement, or a CSV
/// of positions.
pub fn read(content: &str) -> Result<Read, String> {
    let content = content.trim_start_matches('\u{feff}');
    let head = content.get(..content.len().min(200_000)).unwrap_or(content).to_ascii_uppercase();
    if head.contains("<INVPOSLIST>") || head.contains("<INVSTMTRS>") {
        ofx(content)
    } else if head.contains("<OFX>") || head.contains("OFXHEADER") {
        // A bank statement's file: it lists transactions, not holdings.
        Err(not_found())
    } else {
        csv(content)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rows(read: &Read) -> Vec<(&str, &str, &str, &str, &str, &str, &str)> {
        read.positions.iter().map(|p| (p.symbol.as_str(), p.name.as_str(), p.kind.as_str(), p.quantity.as_str(), p.price.as_str(), p.value.as_str(), p.cost.as_str())).collect()
    }

    #[test]
    fn a_brokers_figures_are_read_however_they_are_written() {
        let read = |s: &str| number(s).map(Dec::text);
        assert_eq!(read("$1,234.56").as_deref(), Some("1234.56"));
        assert_eq!(read("1.234,56 €").as_deref(), Some("1234.56"));
        assert_eq!(read("(12.34)").as_deref(), Some("-12.34"));
        assert_eq!(read("-12,5").as_deref(), Some("-12.5"));
        assert_eq!(read("1 234,5").as_deref(), Some("1234.5"));
        assert_eq!(read("1,234").as_deref(), Some("1234"));
        assert_eq!(read("120.5").as_deref(), Some("120.5"));
        assert_eq!(read("SAR 27").as_deref(), Some("27"));
        // Nothing to read is none, never zero.
        assert_eq!((read("--"), read("n/a"), read("")), (None, None, None));
    }

    #[test]
    fn a_positions_csv_is_read_by_its_header_past_the_preamble_and_without_its_totals() {
        let file = "\"Positions for account Brokerage ...123 as of 10:14 AM ET, 2026/11/13\"\n\n\
            \"Symbol\",\"Description\",\"Quantity\",\"Price\",\"Market Value\",\"Day Change %\",\"Cost Basis\",\"Security Type\"\n\
            \"VWRA\",\"VANG FTSE AW USDA\",\"120.5\",\"$141.91\",\"$17,100.00\",\"0.4%\",\"$15,000.00\",\"ETFs & Closed End Funds\"\n\
            \"AAPL\",\"APPLE INC\",\"25\",\"$232.00\",\"$5,800.00\",\"-1.2%\",\"$4,500.00\",\"Equity\"\n\
            \"T 4 1/4\",\"US TREASURY NOTE\",\"5,000\",\"98.5\",\"$4,925.00\",\"0%\",\"--\",\"Fixed Income\"\n\
            \"Cash & Cash Investments\",\"--\",\"--\",\"--\",\"$820.00\",\"0%\",\"--\",\"Cash and Money Market\"\n\
            \"Account Total\",\"--\",\"--\",\"--\",\"$28,645.00\",\"0.1%\",\"$19,500.00\",\"--\"\n";
        let read = read(file).unwrap();
        assert_eq!(rows(&read), vec![
            ("VWRA", "VANG FTSE AW USDA", "FUND", "120.5", "141.91", "17100", "15000"),
            ("AAPL", "APPLE INC", "STK", "25", "232", "5800", "4500"),
            // No cost from the broker is no cost, not zero.
            ("T 4 1/4", "US TREASURY NOTE", "BOND", "5000", "98.5", "4925", ""),
        ]);
        assert_eq!((read.format.as_str(), read.cash.as_str(), read.value.as_str(), read.skipped), ("csv", "820", "28645", 0));
        assert_eq!((read.as_of.as_str(), read.currency.as_str()), ("", ""));
    }

    #[test]
    fn other_layouts_and_languages_read_too() {
        // Semicolons and decimal commas, a value worked out from quantity and price, a cost per share.
        let german = "ISIN;Bezeichnung;Stück;Kurs;Einstandskurs;Währung\nIE00BK5BQT80;Vanguard FTSE All-World;10;105,50;90,00;EUR\nDE0007164600;SAP SE;3;180,00;;EUR\n";
        let read_de = read(german).unwrap();
        assert_eq!(rows(&read_de), vec![
            ("IE00BK5BQT80", "Vanguard FTSE All-World", "", "10", "105.5", "1055", "900"),
            ("DE0007164600", "SAP SE", "", "3", "180", "540", ""),
        ]);
        assert_eq!((read_de.currency.as_str(), read_de.value.as_str()), ("EUR", "1595"));
        // Arabic headers, with the value's currency in brackets.
        let arabic = "الرمز,الاسم,الكمية,السعر,القيمة السوقية (SAR),التكلفة\n2222,أرامكو السعودية,400,27,\"10,800.00\",\"12,000.00\"\n";
        let read_ar = read(arabic).unwrap();
        assert_eq!(rows(&read_ar), vec![("2222", "أرامكو السعودية", "", "400", "27", "10800", "12000")]);
        assert_eq!(read_ar.currency, "SAR");
        // A holding whose value can't be read is counted, not guessed.
        let partial = "Name,Shares,Price,Value\nFund A,10,n/a,n/a\nFund B,2,50,100\n";
        let read_partial = read(partial).unwrap();
        assert_eq!((read_partial.positions.len(), read_partial.skipped), (1, 1));
    }

    #[test]
    fn a_file_without_holdings_says_what_it_looks_for() {
        for file in ["", "Date,Payee,Amount\n2026-11-01,Shop,-12.50\n", "just some words", "OFXHEADER:100\n<OFX><BANKMSGSRSV1><STMTTRN><TRNAMT>-5</STMTTRN></OFX>"] {
            assert!(read(file).unwrap_err().contains("couldn't find holdings"), "{file}");
        }
    }

    #[test]
    fn an_ofx_investment_statement_gives_positions_cash_and_its_day() {
        let file = "OFXHEADER:100\n<OFX><INVSTMTMSGSRSV1><INVSTMTTRNRS><INVSTMTRS><DTASOF>20261113160000.000[-5:EST]<CURDEF>USD\
            <INVPOSLIST>\
            <POSSTOCK><INVPOS><SECID><UNIQUEID>037833100<UNIQUEIDTYPE>CUSIP</SECID><HELDINACCT>CASH<POSTYPE>LONG<UNITS>25<UNITPRICE>232.00<MKTVAL>5800.00<DTPRICEASOF>20261113</INVPOS></POSSTOCK>\
            <POSMF><INVPOS><SECID><UNIQUEID>922908363<UNIQUEIDTYPE>CUSIP</SECID><UNITS>10.5<UNITPRICE>100<MKTVAL>1050<DTPRICEASOF>20261112</INVPOS></POSMF>\
            <POSDEBT><INVPOS><SECID><UNIQUEID>91282CJJ1<UNIQUEIDTYPE>CUSIP</SECID><UNITS>5000<UNITPRICE>98.5<MKTVAL>4925.00</INVPOS></POSDEBT>\
            </INVPOSLIST><INVBAL><AVAILCASH>820.00<MARGINBALANCE>0<SHORTBALANCE>0</INVBAL></INVSTMTRS></INVSTMTTRNRS></INVSTMTMSGSRSV1>\
            <SECLISTMSGSRSV1><SECLIST>\
            <STOCKINFO><SECINFO><SECID><UNIQUEID>037833100<UNIQUEIDTYPE>CUSIP</SECID><SECNAME>APPLE INC<TICKER>AAPL</SECINFO></STOCKINFO>\
            <MFINFO><SECINFO><SECID><UNIQUEID>922908363<UNIQUEIDTYPE>CUSIP</SECID><SECNAME>VANGUARD 500 INDEX &amp; CO<TICKER>VFIAX</SECINFO></MFINFO>\
            </SECLIST></SECLISTMSGSRSV1></OFX>";
        let read = read(file).unwrap();
        assert_eq!(rows(&read), vec![
            ("AAPL", "APPLE INC", "STK", "25", "232", "5800", ""),
            ("VFIAX", "VANGUARD 500 INDEX & CO", "FUND", "10.5", "100", "1050", ""),
            // A security the file doesn't describe is called by its id.
            ("91282CJJ1", "", "BOND", "5000", "98.5", "4925", ""),
        ]);
        assert_eq!((read.format.as_str(), read.as_of.as_str(), read.currency.as_str(), read.cash.as_str(), read.value.as_str()), ("ofx", "2026-11-13", "USD", "820", "12595"));
    }
}
