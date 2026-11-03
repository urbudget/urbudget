"""More cases for make_sms_live_cases.py: many varied texts per format, awkward cases in several
languages, and months of texts. Imported by it; every text is made up.
"""
import random


def vary(bank, want, spend, text):
    """Sets with one or two texts to learn a format from, then eight more in it that differ in every way it allows."""
    payees_en = ["7-ELEVEN 34012", "MCDONALD'S F1234", "THE HOME DEPOT #6512 SAN FRANCISCO CA", "BED BATH & BEYOND", "APPLE.COM/BILL",
                 "SQ *JOE'S PIZZA", "AMZN Mktp US*2K4LL0", "CAFE DE LA PAIX"]
    amounts_us = [("0.99", "0.99"), ("5.00", "5.00"), ("12.30", "12.30"), ("99.99", "99.99"), ("1,000.00", "1000.00"), ("12,345.67", "12345.67"), ("250.00", "250.00"), ("7.25", "7.25")]
    days = [(1, "2026-12-01", "2026-12-01 09:00"), (9, "2026-12-09", "2026-12-09 12:00"), (15, "2026-12-15", "2026-12-15 08:30"), (28, "2026-12-28", "2026-12-28 19:45"),
            (30, "2026-12-30", "2026-12-30 21:10"), (31, "2026-12-31", "2027-01-01 00:20"), (2, "2026-12-02", "2026-12-02 07:05"), (19, "2026-12-19", "2026-12-19 16:00")]

    def us(payee, amount, day):
        d = int(day[1][8:])
        return f"You made a ${amount} transaction with {payee} on your card ending 9876 on 12/{d:02d}."
    bank("USD-VARY", "USD", [("Checking 3301", "checking"), ("Visa 9876", "creditCard")], [
        text("BANK-US", "2026-11-28 10:00", "You made a $18.40 transaction with TARGET 00012345 on your card ending 9876 on 11/28.", spend("18.40", "USD", "2026-11-28", "9876", "target")),
    ], [text("BANK-US", day[2], us(p, a[0], day), spend(a[1], "USD", day[1], "9876", *k))
        for p, a, day, k in zip(payees_en, amounts_us, days, [("7-eleven", "seven"), ("mcdonald",), ("home depot",), ("bed bath",), ("apple",),
                                                               ("joe",), ("amazon", "amzn"), ("paix",)])])

    payees_uk = ["TESCO STORES 3041", "NANDO'S LEEDS", "M&S SIMPLY FOOD", "TFL TRAVEL CH", "AMAZON.CO.UK*1A2B3C", "BOOTS 1234", "CAFFE NERO", "PAYPAL *EBAY"]
    amounts_uk = [("0.99", "0.99"), ("3.40", "3.40"), ("18.00", "18.00"), ("2,345.67", "2345.67"), ("45.10", "45.10"), ("7.99", "7.99"), ("150.00", "150.00"), ("12.00", "12.00")]
    key_uk = ["tesco", "nando", "m&s", "tfl", "amazon", "boots", "nero", "ebay"]
    bank("GBP-VARY", "GBP", [("Current Account 2210", "checking"), ("Credit Card 4512", "creditCard")], [
        text("BANK-UK", "2026-11-28 10:00", "You've spent £9.50 at SAINSBURYS 0223 with your card ending 4512.", spend("9.50", "GBP", "2026-11-28", "4512", "sainsbury")),
    ], [text("BANK-UK", day[2], f"You've spent £{a[0]} at {p} with your card ending 4512.", spend(a[1], "GBP", day[2][:10], "4512", k))
        for p, a, k, day in zip(payees_uk, amounts_uk, key_uk, days)])

    payees_de = ["REWE MARKT", "DM DROGERIE MARKT", "AUTOHAUS MUELLER GMBH & CO KG", "DB VERTRIEB GMBH", "APPLE.COM/BILL", "IKEA", "LIDL SAGT DANKE", "BÄCKEREI KAMPS"]
    amounts_de = [("12,50", "12.50"), ("0,99", "0.99"), ("10.249,00", "10249.00"), ("3,20", "3.20"), ("245,00", "245.00"), ("1.199,99", "1199.99"), ("57,80", "57.80"), ("4,10", "4.10")]
    key_de = ["rewe", "dm", "mueller", "db", "apple", "ikea", "lidl", "kamps"]
    bank("EUR-VARY", "EUR", [("Girokonto 5521", "checking"), ("Kreditkarte 4421", "creditCard")], [
        text("BANK-DE", "2026-11-28 10:00", "Kartenzahlung 23,90 EUR bei EDEKA CENTER, Karte ...4421, 28.11.2026", spend("23.90", "EUR", "2026-11-28", "4421", "edeka")),
        text("BANK-FR", "2026-11-28 12:00", "Paiement CB de 8,40 € chez MONOPRIX le 28/11 carte ****4421", spend("8.40", "EUR", "2026-11-28", "4421", "monoprix")),
    ], [text("BANK-DE", day[2], f"Kartenzahlung {a[0]} EUR bei {p}, Karte ...4421, {day[1][8:]}.{day[1][5:7]}.{day[1][:4]}", spend(a[1], "EUR", day[1], "4421", k))
        for p, a, k, day in zip(payees_de, amounts_de, key_de, days)]
      + [text("BANK-FR", d[2], f"Paiement CB de {a} € chez {p} le {d[1][8:]}/{d[1][5:7]} carte ****4421", spend(v, "EUR", d[1], "4421", k))
         for (p, a, v, k), d in zip([("FNAC", "1 249,00", "1249.00", "fnac"), ("SNCF CONNECT", "89,00", "89.00", "sncf"), ("CARREFOUR MARKET", "0,95", "0.95", "carrefour"),
                                       ("LA POSTE", "12,60", "12.60", "poste")], days[:4])])

    vpas = [("zomato@hdfcbank", "zomato"), ("paytm.s123@hdfcbank", "paytm"), ("blinkit@axisbank", "blinkit"), ("bookmyshow@icici", "bookmyshow"),
            ("irctc@sbi", "irctc"), ("uber.india@axisbank", "uber"), ("bigbasket@icici", "bigbasket"), ("9876543210@ybl", None)]
    amounts_in = [("45.00", "45.00"), ("1,00,000.00", "100000.00"), ("799.00", "799.00"), ("2,345.50", "2345.50"), ("12.00", "12.00"), ("650.75", "650.75"), ("3,999.00", "3999.00"), ("150.00", "150.00")]
    months = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"]
    def mon(iso): return f"{iso[8:]}-{months[int(iso[5:7]) - 1]}-{iso[2:4]}"
    bank("INR-VARY", "INR", [("Savings A/c 1234", "savings"), ("Credit Card 5678", "creditCard")], [
        text("BANK-IN", "2026-11-28 10:00", "Rs.320.00 debited from A/c XX1234 on 28-Nov-26 to VPA swiggy@icici (UPI Ref No 630111222333). Not you? Call 18002586161", spend("320.00", "INR", "2026-11-28", "1234", "swiggy")),
    ], [text("BANK-IN", d[2], f"Rs.{a[0]} debited from A/c XX1234 on {mon(d[1])} to VPA {v} (UPI Ref No 6301{i:08d}). Not you? Call 18002586161", spend(a[1], "INR", d[1], "1234", *( [k] if k else [])))
        for i, ((v, k), a, d) in enumerate(zip(vpas, amounts_in, days))])

    payees_br = [("IFOOD", "ifood"), ("MAGAZINE LUIZA", "magazine"), ("POSTO IPIRANGA 123", "ipiranga"), ("PADARIA SÃO JOÃO", "padaria"), ("NETFLIX.COM", "netflix"), ("DROGASIL 0456", "drogasil")]
    amounts_br = [("45,90", "45.90"), ("1.234,56", "1234.56"), ("200,00", "200.00"), ("8,50", "8.50"), ("55,90", "55.90"), ("12.999,00", "12999.00")]
    bank("BRL-VARY", "BRL", [("Conta 2233", "checking"), ("Cartao 5566", "creditCard")], [
        text("BANK-BR", "2026-11-28 15:00", "Compra aprovada no cartão final 5566 de R$ 39,90 em SPOTIFY em 28/11.", spend("39.90", "BRL", "2026-11-28", "5566", "spotify")),
    ], [text("BANK-BR", d[2], f"Compra aprovada no cartão final 5566 de R$ {a} em {p} em {d[1][8:]}/{d[1][5:7]}.", spend(v, "BRL", d[1], "5566", k))
        for (p, k), (a, v), d in zip(payees_br, amounts_br, days)])

    payees_jp = [("ローソン", "ローソン", "lawson"), ("セブンイレブン", "セブン", "seven"), ("ファミリーマート", "ファミリー", "family"), ("ＡＭＡＺＯＮ．ＣＯ．ＪＰ", "amazon", "ａｍａｚｏｎ"),
                 ("ユニクロ", "ユニクロ", "uniqlo"), ("スターバックス", "スターバックス", "starbucks")]
    amounts_jp = [("640", "640"), ("１，５００", "1500"), ("12,800", "12800"), ("98", "98"), ("３，９９０", "3990"), ("550", "550")]
    bank("JPY-VARY", "JPY", [("カード 7890", "creditCard")], [
        text("BANK-JP", "2026-11-28 12:30", "【カード利用のお知らせ】カード下4桁7890 11/28 マクドナルド 780円", spend("780", "JPY", "2026-11-28", "7890", "マクドナルド", "mcdonald")),
    ], [text("BANK-JP", d[2], f"【カード利用のお知らせ】カード下4桁7890 {d[1][5:7]}/{d[1][8:]} {p} {a}円", spend(v, "JPY", d[1], "7890", k1.lower(), k2.lower()))
        for (p, k1, k2), (a, v), d in zip(payees_jp, amounts_jp, days)])

    arabic = "٠١٢٣٤٥٦٧٨٩"
    def ar(s): return "".join(arabic[int(c)] if c.isdigit() else c for c in s)
    payees_eg = [("سبينيس", "سبينيس"), ("مترو ماركت", "مترو"), ("فودافون مصر", "فودافون"), ("صيدلية العزبي", "العزبي")]
    amounts_eg = [("85٫00", "85.00"), ("1٬250٫50", "1250.50"), ("300", "300.00"), ("12٫75", "12.75")]
    bank("EGP-VARY", "EGP", [("Account 4567", "checking")], [
        text("BANK-EG", "2026-11-28 13:00", ar("تم خصم ") + ar("45") + "٫" + ar("00") + " جنيه من حسابكم رقم ****" + ar("4567") + " لدى كارفور بتاريخ " + ar("28/11/2026"),
             spend("45.00", "EGP", "2026-11-28", "4567", "كارفور", "carrefour")),
    ], [text("BANK-EG", d[2], "تم خصم " + ar(a) + " جنيه من حسابكم رقم ****" + ar("4567") + " لدى " + p + " بتاريخ " + ar(f"{d[1][8:]}/{d[1][5:7]}/{d[1][:4]}"),
             spend(v, "EGP", d[1], "4567", k)) for (p, k), (a, v), d in zip(payees_eg, amounts_eg, days)])

    def anb(amount, payee, day, balance):
        lines = ["Online Purchase", "VISA Card:4512", f"Amount:SAR {amount}"] + ([f"Balance:SAR {balance}"] if balance else []) + [f"At:{payee}", f"On:{day[1][8:]}/{day[1][5:7]}/{day[1][2:4]} {day[2][11:]}"]
        return "\n".join(lines)
    payees_sa = [("NOON E", "noon"), ("AMAZON SA", "amazon"), ("JARIR BOOKSTORE", "jarir"), ("HUNGERSTATION L", "hungerstation"), ("STC PAY", "stc"), ("TAMIMI MARKETS", "tamimi")]
    amounts_sa = [("65.97", "65.97"), ("1,234.50", "1234.50"), ("9.00", "9.00"), ("320.25", "320.25"), ("15.00", "15.00"), ("2,999.99", "2999.99")]
    bank("SAR-VARY", "SAR", [("Current 7731", "checking"), ("Visa 4512", "creditCard")], [
        # A line that's sometimes there: both kinds to learn from.
        text("BANK-SA", "2026-11-28 15:01", anb("45.00", "PANDA RET", (0, "2026-11-28", "2026-11-28 15:01"), "9,120.40"), spend("45.00", "SAR", "2026-11-28", "4512", "panda")),
        text("BANK-SA", "2026-11-28 18:20", anb("12.00", "STARBUCKS", (0, "2026-11-28", "2026-11-28 18:20"), None), spend("12.00", "SAR", "2026-11-28", "4512", "starbucks")),
    ], [text("BANK-SA", d[2], anb(a, p, d, "8,000.00" if i % 2 else None), spend(v, "SAR", d[1], "4512", k))
        for i, ((p, k), (a, v), d) in enumerate(zip(payees_sa, amounts_sa, days))])

    names = [("PETER KAMAU", "peter"), ("MARY WANJIKU NJERI", "mary"), ("JAMES OTIENO", "james"), ("GRACE AKINYI", "grace")]
    amounts_ke = [("1,200.00", "1200.00"), ("50.00", "50.00"), ("12,500.00", "12500.00"), ("350.00", "350.00")]
    bank("KES-VARY", "KES", [("M-Pesa", "cash")], [
        text("MPESA", "2026-11-28 14:15", "QK12AB34CD Confirmed. Ksh500.00 sent to JOHN DOE 0712345678 on 28/11/26 at 2:15 PM. New M-PESA balance is Ksh1,234.00. Transaction cost, Ksh7.00.", spend("500.00", "KES", "2026-11-28", None, "john")),
    ], [text("MPESA", d[2], f"QL{i}5MN66OP Confirmed. Ksh{a} sent to {n} 07{i}3111222 on {int(d[1][8:])}/12/26 at {int(d[2][11:13]) % 12 or 12}:{d[2][14:16]} {'PM' if int(d[2][11:13]) >= 12 else 'AM'}. New M-PESA balance is Ksh534.00. Transaction cost, Ksh13.00.",
             spend(v, "KES", d[1], None, k)) for i, ((n, k), (a, v), d) in enumerate(zip(names, amounts_ke, days))])


def hard(bank, want, spend, text):
    """Awkward cases in Arabic, Spanish, French and Indian English."""
    bank("SAR-HARD", "SAR", [("Current 3301", "checking"), ("Visa 4512", "creditCard")], [
        text("BANK-SA", "2026-11-04 10:00", "تم عكس عملية شراء بمبلغ 120.00 ريال لدى نون على البطاقة 4512", want("skip:declined|refund", "120.00", "SAR", None, "4512")),
        text("BANK-SA", "2026-11-04 11:00", "تم استرداد مبلغ 35.00 ريال جزئياً من أمازون إلى بطاقتك 4512", want("refund", "35.00", "SAR", "2026-11-04", "4512", ["amazon", "أمازون"])),
        text("BANK-SA", "2026-11-04 12:00", "تعذر تنفيذ حوالة بمبلغ 1,000.00 ريال من الحساب 3301", want("skip:declined")),
        text("BANK-SA", "2026-11-04 13:00", "رصيد حسابك 3301 أقل من 500 ريال: الرصيد الحالي 432.10 ريال", want("skip:balance")),
        text("BANK-SA", "2026-11-04 14:00", "تم تقسيط مشترياتك بمبلغ 3,600.00 ريال على 12 شهراً بقسط 300.00 ريال", want("skip:other|skip:balance")),
        text("BANK-SA", "2026-11-04 15:00", "شراء دولي بمبلغ USD 50.00 لدى AMAZON.COM بطاقة 4512، المبلغ بالريال 191.25 شاملاً رسوم 3.75", want("expense", "-191.25", "USD", "2026-11-04", "4512", ["amazon"])),
        text("BANK-SA", "2026-11-04 16:00", "تم استلام سداد بطاقتك 4512 بمبلغ 2,000.00 ريال", want("transfer", "2000.00", "SAR", "2026-11-04", "4512")),
        text("BANK-SA", "2026-11-04 17:00", "حصلت على 120 نقطة من مشترياتك هذا الشهر", want("skip:rewards")),
        text("BANK-SA", "2026-11-04 18:00", "تم إيداع استرداد نقدي 15.00 ريال في بطاقتك 4512", want("income|refund", "15.00", "SAR", "2026-11-04", "4512")),
        text("BANK-SA", "2026-11-04 19:00", "عملياتك اليوم على البطاقة 4512: 25.00 ريال ستاربكس، 80.00 ريال جرير", want("unclear")),
        # A salary paid in two parts: two items.
        text("BANK-SA", "2026-11-25 05:00", "حوالة واردة: راتب الجزء الأول مبلغ: 6000 ريال إلى : ***3301", want("income", "6000.00", "SAR", "2026-11-25", "3301")),
        text("BANK-SA", "2026-11-25 05:01", "حوالة واردة: راتب الجزء الثاني مبلغ: 6000 ريال إلى : ***3301", want("income", "6000.00", "SAR", "2026-11-25", "3301")),
    ])
    bank("EUR-HARD", "EUR", [("Cuenta 5521", "checking"), ("Tarjeta 4421", "creditCard")], [
        text("BANK-ES", "2026-11-04 10:00", "Devolución parcial de 12,00 EUR de ZARA en tarjeta 4421 el 04/11/2026", want("refund", "12.00", "EUR", "2026-11-04", "4421", ["zara"])),
        text("BANK-ES", "2026-11-04 11:00", "Operación anulada: compra de 45,90 EUR en EL CORTE INGLES con tarjeta 4421", want("skip:declined|refund", "45.90", "EUR", None, "4421")),
        text("BANK-ES", "2026-11-04 12:00", "Recibo devuelto: 49,99 EUR de VODAFONE ESPAÑA, cuenta 5521. Comisión de devolución 3,00 EUR.", want("unclear|expense", None, "", None, "5521")),
        text("BANK-FR", "2026-11-04 13:00", "Prélèvement rejeté de 89,00 € SFR sur le compte 5521, frais de rejet 20,00 €", want("unclear|expense", None, "", None, "5521")),
        text("BANK-FR", "2026-11-04 14:00", "Votre paiement de 1 200,00 € chez DARTY a été converti en 4 fois sans frais de 300,00 €", want("skip:other|skip:balance")),
        text("BANK-FR", "2026-11-04 15:00", "Remboursement de 15,90 € de FNAC sur votre carte ****4421 le 04/11", want("refund", "15.90", "EUR", "2026-11-04", "4421", ["fnac"])),
        text("BANK-FR", "2026-11-04 16:00", "Virement reçu de 2 500,00 € de ACME SAS sur le compte 5521", want("income", "2500.00", "EUR", "2026-11-04", "5521")),
        text("BANK-FR", "2026-11-04 17:00", "Solde de votre compte 5521 : 1 234,56 €", want("skip:balance")),
    ])
    bank("INR-HARD", "INR", [("Savings A/c 1234", "savings"), ("Credit Card 5678", "creditCard")], [
        text("BANK-IN", "2026-11-04 10:00", "Rs.2,000.00 reversed to A/c XX1234 for failed UPI txn 630123456789 on 04-Nov-26", want("refund|income", "2000.00", "INR", "2026-11-04", "1234")),
        text("BANK-IN", "2026-11-04 11:00", "Your SIP of Rs.5,000 in AXIS BLUECHIP FUND has been processed from A/c XX1234", want("expense|transfer", "-5000.00", "INR", "2026-11-04", "1234")),
        text("BANK-IN", "2026-11-04 12:00", "Cheque No 000123 for Rs.25,000 deposited in A/c XX1234 has been returned unpaid. Charges Rs.590 applied.", want("unclear|expense", None, "", None, "1234")),
        text("BANK-IN", "2026-11-04 13:00", "Rs.1,500.00 cashback credited to Credit Card XX5678 for AMAZON purchase", want("income|refund", "1500.00", "INR", "2026-11-04", "5678")),
        text("BANK-IN", "2026-11-04 14:00", "Your credit card XX5678 payment of Rs.18,000.00 has been received.", want("transfer", "18000.00", "INR", "2026-11-04", "5678")),
        text("BANK-IN", "2026-11-04 15:00", "Avl Bal in A/c XX1234 is Rs.12,345.67 as on 04-Nov-26", want("skip:balance")),
    ])


def months(want, spend):
    """Two months of texts each for a US, an Indian and a Saudi profile: mostly junk, as a phone gets them."""
    rng = random.Random(42)
    out = []

    def text(sender, at, body, w):
        return {"sender": sender, "at": at, "body": body, "want": w}

    def month_days(year, month, n):
        days = sorted(rng.sample(range(1, 29), min(n, 28)) * (n // 28 + 1))[:n]
        return [(f"{year}-{month:02d}-{d:02d}", f"{rng.randint(7, 22):02d}:{rng.randint(0, 59):02d}") for d in days]

    def us(year, month):
        payees = ["TARGET 00012345", "STARBUCKS STORE 0412", "SHELL OIL 5774", "TRADER JOE'S #123", "CVS/PHARMACY #4521", "UBER *TRIP", "CHIPOTLE 2045",
                  "WHOLE FOODS MKT", "SPOTIFY USA", "COSTCO WHSE #0412", "HOME DEPOT #6512", "AMAZON MKTPLACE PMTS"]
        texts = []
        for n, (day, time) in enumerate(month_days(year, month, 30)):
            p = rng.choice(payees); a = f"{rng.randint(3, 240)}.{rng.randint(0, 99):02d}"
            at = f"{day} {time}"
            if n % 2 == 0:
                texts.append(text("BANK-US", day + " " + f"{int(time[:2]):02d}:{max(int(time[3:]) - 1, 0):02d}", f"{rng.randint(100000, 999999)} is your verification code for a purchase of ${a} at {p}. Do not share it.", want("skip:code")))
            texts.append(text("BANK-US", at, f"You made a ${a} transaction with {p} on your card ending 9876 on {day[5:7]}/{day[8:]}.", spend(a, "USD", day, "9876")))
        for day in [f"{year}-{month:02d}-01", f"{year}-{month:02d}-15"]:
            texts.append(text("BANK-US", day + " 06:00", f"A $2,450.00 direct deposit from ACME PAYROLL posted to checking ***3301 on {day[5:7]}/{day[8:]}.", want("income", "2450.00", "USD", day, "3301")))
        texts.append(text("BANK-US", f"{year}-{month:02d}-20 10:00", f"Payment of $1,820.45 from checking ***3301 to your credit card ending 9876 posted {month:02d}/20/{str(year)[2:]}", want("transfer", "-1820.45", "USD", f"{year}-{month:02d}-20", "3301", other="9876")))
        for day, time in month_days(year, month, 10):
            texts.append(text("BANK-US", f"{day} {time}", f"New sign-in to your account from {rng.choice(['Chrome on Windows', 'Safari on iPhone', 'the mobile app'])}. Wasn't you? Call us.", want("skip:security")))
        for day, time in month_days(year, month, 25):
            texts.append(text("BANK-US", f"{day} {time}", f"Get {rng.choice([3, 5, 10])}% cashback at {rng.choice(payees)} this weekend! Terms apply. Reply STOP to opt out.", want("skip:marketing")))
        for day, time in month_days(year, month, 6):
            texts.append(text("BANK-US", f"{day} {time}", f"Checking ***3301 balance is ${rng.randint(50, 99)}.{rng.randint(0, 99):02d}, below your $100.00 alert.", want("skip:balance")))
        return sorted(texts, key=lambda t: t["at"])

    def india(year, month):
        vpas = ["swiggy@icici", "zomato@hdfcbank", "blinkit@axisbank", "irctc@sbi", "bigbasket@icici", "uber.india@axisbank"]
        texts = []
        names = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"]
        for n, (day, time) in enumerate(month_days(year, month, 30)):
            v = rng.choice(vpas); a = f"{rng.randint(20, 2400)}.00"
            short = f"{day[8:]}-{names[month - 1]}-{str(year)[2:]}"
            if n % 3 == 0:
                texts.append(text("BANK-IN", f"{day} {time}", f"Rs.{a} spent on card XX5678 at {v.split('@')[0].upper()} on {short}.", spend(a, "INR", day, "5678")))
            else:
                texts.append(text("BANK-IN", f"{day} {time}", f"Rs.{a} debited from A/c XX1234 on {short} to VPA {v} (UPI Ref No 6301{rng.randint(10000000, 99999999)}). Not you? Call 18002586161", spend(a, "INR", day, "1234")))
            if n % 3 == 0:
                texts.append(text("BANK-IN", f"{day} {time[:3]}{max(int(time[3:]) - 2, 0):02d}", f"{rng.randint(100000, 999999)} is your OTP for txn of INR {a} at {v.split('@')[0].upper()} on card XX5678. Valid for 10 mins. Do not share.", want("skip:code")))
        day = f"{year}-{month:02d}-01"
        texts.append(text("BANK-IN", day + " 09:00", f"INR 1,23,456.00 credited to A/c XX1234 on 01-{names[month - 1]}-{str(year)[2:]} by NEFT from ACME TECHNOLOGIES PVT LTD. Avl Bal INR 2,45,000.50", want("income", "123456.00", "INR", day, "1234")))
        for day, time in month_days(year, month, 20):
            texts.append(text("BANK-IN", f"{day} {time}", f"Get a pre-approved personal loan of Rs.{rng.choice(['5,00,000', '10,00,000'])} in 2 minutes! Apply now: bnk.in/pl T&C", want("skip:marketing")))
        for day, time in month_days(year, month, 12):
            texts.append(text("BANK-IN", f"{day} {time}", f"Avl Bal in A/c XX1234 is Rs.{rng.randint(10000, 90000)}.{rng.randint(0, 99):02d} as on {day[8:]}-{names[month - 1]}-{str(year)[2:]}", want("skip:balance")))
        return sorted(texts, key=lambda t: t["at"])

    def saudi(year, month):
        payees = ["NOON E", "AMAZON SA", "JARIR BOOKSTORE", "HUNGERSTATION L", "PANDA RET", "TAMIMI MARKETS", "STARBUCKS", "ALDREES 12"]
        texts = []
        for n, (day, time) in enumerate(month_days(year, month, 30)):
            p = rng.choice(payees); a = f"{rng.randint(5, 900)}.{rng.randint(0, 99):02d}"
            d = f"{day[8:]}/{day[5:7]}/{day[2:4]}"
            if n % 2:
                texts.append(text("BANK-SA", f"{day} {time}", f"Online Purchase\nVISA Card:4512\nAmount:SAR {a}\nAt:{p}\nOn:{d} {time}", spend(a, "SAR", day, "4512")))
            else:
                texts.append(text("BANK-SA", f"{day} {time}", f"شراء عبر نقاط البيع\nبـ:SAR {a}\nمن:{p}\nمدى:7731\nفي:{d} {time}", spend(a, "SAR", day, "7731")))
                texts.append(text("BANK-SA", f"{day} {time[:3]}{max(int(time[3:]) - 1, 0):02d}", f"لـ: شراء\nلا تشارك الرمز\nرمز: {rng.randint(1000, 9999)}\nمبلغ: SAR {a}", want("skip:code")))
        day = f"{year}-{month:02d}-27"
        texts.append(text("BANK-SA", day + " 03:10", "حوالة واردة: راتب\nمبلغ: 35000 ريال\nإلى : ***7731", want("income", "35000.00", "SAR", day, "7731")))
        for day, time in month_days(year, month, 12):
            texts.append(text("BANK-SA", f"{day} {time}", "اشعار: تم الدخول بخاصية البصمة في التطبيق", want("skip:security")))
        for day, time in month_days(year, month, 15):
            texts.append(text("BANK-SA", f"{day} {time}", f"The Earned miles has been credited to your Al Fursan Membership No {rng.randint(10000000, 99999999)}\nNumber of Earned miles as per daily spend : {rng.randint(5, 200)}\nVISA Credit Card", want("skip:rewards")))
        for day, time in month_days(year, month, 15):
            texts.append(text("BANK-SA", f"{day} {time}", f"عرض خاص: استرداد نقدي {rng.choice([5, 10, 15])}% عند الشراء من {rng.choice(payees)} هذا الأسبوع. تطبق الشروط", want("skip:marketing")))
        return sorted(texts, key=lambda t: t["at"])

    for name, currency, accounts, make in [
        ("US-MONTH", "USD", [("Checking 3301", "checking"), ("Visa 9876", "creditCard")], us),
        ("IN-MONTH", "INR", [("Savings A/c 1234", "savings"), ("Credit Card 5678", "creditCard")], india),
        ("SA-MONTH", "SAR", [("Current 7731", "checking"), ("Visa 4512", "creditCard")], saudi),
    ]:
        out.append({"name": name, "currency": currency, "accounts": [list(a) for a in accounts], "months": [make(2026, 11), make(2026, 12)]})
    return out
