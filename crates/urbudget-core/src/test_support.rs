//! Deterministic fixtures shared by desktop and core regression tests.
use crate::domain::*;
use serde_json::Map;

pub fn sample() -> Budget {
    Budget {
        accounts: vec![
            Account {
                extra: Map::new(),
                id: "cash".into(),
                name: "Cash".into(),
                kind: AccountKind::Cash,
                opening_balance: 100_000,
                archived: false,
            },
            Account {
                extra: Map::new(),
                id: "card".into(),
                name: "Card".into(),
                kind: AccountKind::Credit,
                opening_balance: -10_000,
                archived: false,
            },
        ],
        categories: vec![
            Category {
                extra: Map::new(),
                id: "food".into(),
                name: "Food".into(),
                group: "Living".into(),
                target: 20_000,
            },
            Category {
                extra: serde_json::json!({"paymentAccountId":"card"})
                    .as_object()
                    .unwrap()
                    .clone(),
                id: "payment".into(),
                name: "Card payment".into(),
                group: "Credit Card Payments".into(),
                target: 0,
            },
        ],
        assignments: vec![Assignment { extra: Default::default(),
            month: "2026-10".into(),
            category_id: "food".into(),
            amount: 20_000,
        }],
        ..Budget::default()
    }
}
pub fn purchase() -> Transaction {
    Transaction {
        extra: Default::default(),
        id: "tx".into(),
        date: "2026-10-15".into(),
        account_id: "card".into(),
        payee: "Shop".into(),
        memo: String::new(),
        amount: -5_000,
        status: Cleared::Cleared,
        splits: vec![Split {
            extra: Default::default(),
            category_id: Some("food".into()),
            amount: -5_000,
        }],
        transfer_account_id: None,
        transfer_status: None,
    }
}

/// A statement the way a query set up from the app's steps sends it: a USD account holding
/// a US stock, a London fund and a Saudi stock, with cash in two currencies.
pub const FLEX_SAMPLE: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<FlexQueryResponse queryName="UrBudget" type="AF">
<FlexStatements count="1">
<FlexStatement accountId="U1234567" fromDate="20260901" toDate="20261113" period="Last365CalendarDays" whenGenerated="20261114;063012">
<AccountInformation accountId="U1234567" acctAlias="Long term" currency="USD" name="Someone Private" accountType="Individual" />
<EquitySummaryInBase>
<EquitySummaryByReportDateInBase accountId="U1234567" reportDate="20260930" cash="6220" stock="18000" dividendAccruals="0" interestAccruals="0" total="24220" />
<EquitySummaryByReportDateInBase accountId="U1234567" reportDate="20261030" cash="1200" stock="24300" dividendAccruals="0" interestAccruals="0" total="25500" />
<EquitySummaryByReportDateInBase accountId="U1234567" reportDate="20261112" cash="1220" stock="25380.5" dividendAccruals="12.5" interestAccruals="0" total="26613" />
<EquitySummaryByReportDateInBase accountId="U1234567" reportDate="20261113" cash="1220" stock="25780" dividendAccruals="12.5" interestAccruals="0" total="27012.5" />
</EquitySummaryInBase>
<CashReport>
<CashReportCurrency accountId="U1234567" currency="BASE_SUMMARY" levelOfDetail="BaseCurrency" fromDate="20260901" toDate="20261113" startingCash="6220" endingCash="1220" endingSettledCash="1220" commissions="-3.5" />
<CashReportCurrency accountId="U1234567" currency="USD" levelOfDetail="Currency" fromDate="20260901" toDate="20261113" startingCash="6220" endingCash="820" endingSettledCash="820" />
<CashReportCurrency accountId="U1234567" currency="SAR" levelOfDetail="Currency" fromDate="20260901" toDate="20261113" startingCash="0" endingCash="1500" endingSettledCash="1500" />
</CashReport>
<OpenPositions>
<OpenPosition accountId="U1234567" currency="USD" fxRateToBase="1" assetCategory="STK" symbol="AAPL" description="APPLE INC" conid="265598" isin="US0378331005" figi="BBG000B9XRY4" listingExchange="NASDAQ" multiplier="1" reportDate="20261113" position="25" markPrice="232" positionValue="5800" costBasisPrice="180" costBasisMoney="4500" fifoPnlUnrealized="1300" side="Long" levelOfDetail="SUMMARY" />
<OpenPosition accountId="U1234567" currency="USD" fxRateToBase="1" assetCategory="STK" symbol="AAPL" description="APPLE INC" conid="265598" isin="US0378331005" listingExchange="NASDAQ" multiplier="1" reportDate="20261113" position="25" markPrice="232" positionValue="5800" costBasisMoney="4500" fifoPnlUnrealized="1300" side="Long" levelOfDetail="LOT" openDateTime="20250110;093000" />
<OpenPosition accountId="U1234567" currency="USD" fxRateToBase="1" assetCategory="STK" symbol="VWRA" description="VANG FTSE AW USDA" conid="338105316" isin="IE00BK5BQT80" listingExchange="LSEETF" multiplier="1" reportDate="20261113" position="120.5" markPrice="141.9087137" positionValue="17100" costBasisMoney="15000" fifoPnlUnrealized="2100" side="Long" levelOfDetail="SUMMARY" />
<OpenPosition accountId="U1234567" currency="SAR" fxRateToBase="0.26666667" assetCategory="STK" symbol="2222" description="SAUDI ARABIAN OIL CO" conid="400000001" isin="SA14TG012N13" listingExchange="TADAWUL" multiplier="1" reportDate="20261113" position="400" markPrice="27" positionValue="10800" costBasisMoney="12000" fifoPnlUnrealized="-1200" side="Long" levelOfDetail="SUMMARY" />
</OpenPositions>
<CashTransactions>
<CashTransaction accountId="U1234567" currency="USD" fxRateToBase="1" description="CASH RECEIPTS / ELECTRONIC FUND TRANSFERS" dateTime="20261005;202000" settleDate="20261005" amount="1000" type="Deposits/Withdrawals" transactionID="9000001" reportDate="20261005" levelOfDetail="DETAIL" />
<CashTransaction accountId="U1234567" currency="SAR" fxRateToBase="0.26666667" description="CASH RECEIPTS / ELECTRONIC FUND TRANSFERS" dateTime="20261102;101500" settleDate="20261102" amount="1500" type="Deposits/Withdrawals" transactionID="9000002" reportDate="20261102" levelOfDetail="DETAIL" />
<CashTransaction accountId="U1234567" currency="USD" fxRateToBase="1" symbol="AAPL" description="AAPL(US0378331005) CASH DIVIDEND USD 0.26 PER SHARE (Ordinary Dividend)" dateTime="20261106;202000" settleDate="20261106" amount="6.5" type="Dividends" transactionID="9000003" reportDate="20261106" levelOfDetail="DETAIL" />
<CashTransaction accountId="U1234567" currency="USD" fxRateToBase="1" symbol="AAPL" description="AAPL(US0378331005) CASH DIVIDEND USD 0.26 PER SHARE - US TAX" dateTime="20261106;202000" settleDate="20261106" amount="-0.98" type="Withholding Tax" transactionID="9000004" reportDate="20261106" levelOfDetail="DETAIL" />
<CashTransaction accountId="U1234567" currency="USD" fxRateToBase="1" description="USD CREDIT INT FOR SEP-2026" dateTime="20261004;202000" settleDate="20261004" amount="1.2" type="Broker Interest Received" transactionID="9000005" reportDate="20261004" levelOfDetail="DETAIL" />
<CashTransaction accountId="U1234567" currency="USD" fxRateToBase="1" description="MARKET DATA FOR SEP 2026" dateTime="20261003;202000" settleDate="20261003" amount="-10" type="Other Fees" transactionID="9000006" reportDate="20261003" levelOfDetail="DETAIL" />
</CashTransactions>
<Transfers />
<ConversionRates>
<ConversionRate reportDate="20260930" fromCurrency="SAR" toCurrency="USD" rate="0.26666667" />
<ConversionRate reportDate="20261030" fromCurrency="SAR" toCurrency="USD" rate="0.26666667" />
<ConversionRate reportDate="20261113" fromCurrency="SAR" toCurrency="USD" rate="0.26666667" />
<ConversionRate reportDate="20261113" fromCurrency="EUR" toCurrency="USD" rate="1.0842" />
</ConversionRates>
</FlexStatement>
</FlexStatements>
</FlexQueryResponse>"#;
