import CoreImage
import XCTest
@testable import UrBudget

/// Account & sync's reading of what the engine says about the sync key
/// (docs/plans/2026-11-24-sync-device-approval.md): the status fields, which rows the screen
/// shows, a recovery code to show, where a shown device code stands, and the QR code's squares.
final class SyncKeyTests: XCTestCase {
    private func status(_ sync: String) throws -> JSON {
        try JSON.parse(#"{"email":"qa@example.test","sync":\#(sync)}"#)
    }

    /// sync.rs `Status` as it's serialized, with the fields this work added.
    func testStatusSaysWhatTheKeyIsMadeFromAndWhetherACodeIsUnsaved() throws {
        let made = try status(#"{"unlocked":true,"protocol":2,"enabled":true,"upgradeAvailable":false,"upgradeNeedsPassphrase":false,"secret":"code","recovery":true,"recoveryUnsaved":true,"last":null}"#)
        XCTAssertEqual(SyncKey.secret(made), "code")
        XCTAssertTrue(SyncKey.hasRecoveryCopy(made))
        XCTAssertTrue(SyncKey.recoveryUnsaved(made))
        XCTAssertTrue(SyncKey.canReplaceRecoveryCode(made))

        let chosen = try status(#"{"unlocked":true,"protocol":1,"enabled":true,"secret":"passphrase","recovery":false,"recoveryUnsaved":false}"#)
        XCTAssertEqual(SyncKey.secret(chosen), "passphrase")
        XCTAssertFalse(SyncKey.hasRecoveryCopy(chosen))
        XCTAssertFalse(SyncKey.recoveryUnsaved(chosen))
        XCTAssertFalse(SyncKey.canReplaceRecoveryCode(chosen), "a replacement code needs the new sync")

        // No key, or nothing heard yet: the engine says null. An older engine says nothing.
        for quiet in [try status(#"{"unlocked":false,"protocol":1,"secret":null,"recovery":false,"recoveryUnsaved":false}"#),
                      try status(#"{"unlocked":false,"protocol":1}"#), JSON.object([:])] {
            XCTAssertEqual(SyncKey.secret(quiet), "")
            XCTAssertFalse(SyncKey.hasRecoveryCopy(quiet))
            XCTAssertFalse(SyncKey.recoveryUnsaved(quiet))
            XCTAssertFalse(SyncKey.canReplaceRecoveryCode(quiet))
        }
        XCTAssertFalse(SyncKey.canReplaceRecoveryCode(try status(#"{"unlocked":false,"protocol":2,"secret":"code"}"#)), "locked")
    }

    /// Turn on sync, the unlock rows or the unlocked ones: `sync_key_info` decides, asked fresh.
    func testTheKeyDecidesWhichRowsShow() throws {
        let locked = try status(#"{"unlocked":false,"protocol":2,"secret":"code","recovery":false}"#)
        let unheard = try status(#"{"unlocked":false,"protocol":1,"secret":null}"#)
        let unlocked = try status(#"{"unlocked":true,"protocol":2,"secret":"code"}"#)
        let none = try JSON.parse(#"{"exists":false,"secret":null,"recovery":false,"protocol":2}"#)
        let code = try JSON.parse(#"{"exists":true,"secret":"code","recovery":false,"protocol":2}"#)
        let passphrase = try JSON.parse(#"{"exists":true,"secret":"passphrase","recovery":true,"protocol":1}"#)

        XCTAssertEqual(SyncKey.stage(unheard, info: nil), .asking)
        XCTAssertEqual(SyncKey.stage(unheard, info: none), .none)
        XCTAssertEqual(SyncKey.stage(unheard, info: code), .locked)
        // Offline before the answer: the last sync heard of a key, so unlocking is offered.
        XCTAssertEqual(SyncKey.stage(locked, info: nil), .locked)
        // A reset elsewhere since: the fresh answer wins over what the last sync saved.
        XCTAssertEqual(SyncKey.stage(locked, info: none), .none)
        XCTAssertEqual(SyncKey.stage(unlocked, info: nil), .unlocked)
        XCTAssertEqual(SyncKey.stage(unlocked, info: none), .unlocked, "this device holds the key whatever was asked before")
        XCTAssertEqual(SyncKey.stage([:], info: nil), .asking)

        // What the box is labelled for: the fresh answer first, then the last sync's word.
        XCTAssertEqual(SyncKey.secret(unheard, info: code), "code")
        XCTAssertEqual(SyncKey.secret(locked, info: passphrase), "passphrase")
        XCTAssertEqual(SyncKey.secret(locked, info: nil), "code")
        XCTAssertEqual(SyncKey.secret(unheard, info: none), "")
        XCTAssertTrue(SyncKey.hasRecoveryCopy(locked, info: passphrase))
        // A reset's wording follows the account's protocol: the fresh answer first here too.
        XCTAssertEqual(SyncKey.protocolNumber(locked), 2)
        XCTAssertEqual(SyncKey.protocolNumber(unheard), 1)
        XCTAssertEqual(SyncKey.protocolNumber(unheard, info: code), 2)
        XCTAssertEqual(SyncKey.protocolNumber(locked, info: passphrase), 1)
        XCTAssertFalse(SyncKey.hasRecoveryCopy(try status(#"{"recovery":true}"#), info: code))
    }

    /// `sync_setup`, `sync_reset`, `sync_recovery_create` and `sync_recovery_unsaved` answer with
    /// a code to show, or without one.
    func testARecoveryCodeIsShownOnlyWhenTheAnswerCarriesOne() throws {
        let made = try JSON.parse(#"{"created":true,"protocol":2,"recoveryCode":"0123-4567-89AB-CDEF-GHJK-MNPQ"}"#)
        XCTAssertEqual(RecoveryCode(made)?.code, "0123-4567-89AB-CDEF-GHJK-MNPQ")
        XCTAssertEqual(RecoveryCode(made)?.id, "0123-4567-89AB-CDEF-GHJK-MNPQ")
        XCTAssertNil(RecoveryCode(try JSON.parse(#"{"created":false,"protocol":2}"#)), "unlocked with a passphrase")
        XCTAssertNil(RecoveryCode(.null), "sync_recovery_unsaved with nothing kept")
        XCTAssertNil(RecoveryCode(["recoveryCode": ""]))
        XCTAssertNil(RecoveryCode(["recoveryCode": .null]))
    }

    /// The real engine, signed out: the status has the new fields, there's no code to show, and
    /// a device code can't be asked for.
    func testTheEngineSignedOutHasNoKeyAndNoCode() async throws {
        _ = try await TestLibrary.repository()
        let status = try NativeCore.service("cloud_status")
        for field in ["secret", "recovery", "recoveryUnsaved"] { XCTAssertTrue(status["sync"].has(field), field) }
        XCTAssertEqual(SyncKey.secret(status), "")
        XCTAssertFalse(SyncKey.hasRecoveryCopy(status))
        XCTAssertFalse(SyncKey.recoveryUnsaved(status))
        XCTAssertEqual(SyncKey.stage(status, info: nil), .asking)
        XCTAssertNil(RecoveryCode(try NativeCore.service("sync_recovery_unsaved")))
        XCTAssertEqual(try NativeCore.service("sync_recovery_saved"), .null)
        XCTAssertThrowsError(try NativeCore.service("sync_pair_request"))
    }

    /// `sync_pair_offer`'s answer, and what `sync_pair_state` then says of it: a device is added
    /// only when the server says the key was collected, never by the clock.
    func testAShownCodeIsCollectedOrRunsOut() throws {
        let now = try XCTUnwrap(SyncKey.date("2026-11-24T10:00:00.000Z"))
        let answer = try JSON.parse(#"{"id":"82a4d067d9aa39431ee15f1626c663e9","code":"0123-4567-89AB-CDEF-GHJK-MNPQ","qr":{"size":21,"modules":"\#(String(repeating: "10", count: 220))1"},"expiresAt":"2026-11-24T10:05:00.000Z"}"#)
        let offer = try XCTUnwrap(PairingOffer(answer, now: now))
        XCTAssertEqual(offer.id, "82a4d067d9aa39431ee15f1626c663e9")
        XCTAssertEqual(offer.code, "0123-4567-89AB-CDEF-GHJK-MNPQ")
        XCTAssertEqual(offer.qr?.size, 21)
        XCTAssertEqual(offer.expiresAt.timeIntervalSince(now), 300)

        let waiting: JSON = ["waiting": true, "collected": false]
        let collected: JSON = ["waiting": false, "collected": true]
        let gone: JSON = ["waiting": false, "collected": false]
        XCTAssertEqual(offer.standing(waiting, now: now), .waiting)
        XCTAssertEqual(offer.standing(collected, now: now.addingTimeInterval(3)), .collected)
        // Ran out, or taken back: a second after it was shown or at the end, it's never a device added.
        XCTAssertEqual(offer.standing(gone, now: now.addingTimeInterval(1)), .expired)
        XCTAssertEqual(offer.standing(gone, now: now.addingTimeInterval(300)), .expired)
        // The server decides while it answers, whatever this device's clock says.
        XCTAssertEqual(offer.standing(waiting, now: now.addingTimeInterval(900)), .waiting)
        XCTAssertEqual(offer.standing(collected, now: now.addingTimeInterval(900)), .collected)
        // No answer (offline): the code stands until its time is up.
        XCTAssertEqual(offer.standing(nil, now: now.addingTimeInterval(299)), .waiting)
        XCTAssertEqual(offer.standing(nil, now: now.addingTimeInterval(300)), .expired)
        // An engine from before `collected` never calls a device added.
        XCTAssertEqual(offer.standing(["waiting": false], now: now.addingTimeInterval(3)), .expired)

        // Times without fractions read too; without one at all, a code lasts five minutes.
        XCTAssertEqual(SyncKey.date("2026-11-24T10:05:00Z"), offer.expiresAt)
        var undated = answer
        undated.remove("expiresAt")
        XCTAssertEqual(PairingOffer(undated, now: now)?.expiresAt, offer.expiresAt)
        // A code with no picture can still be typed.
        var unpictured = answer
        unpictured["qr"] = .null
        XCTAssertNil(try XCTUnwrap(PairingOffer(unpictured, now: now)).qr)
        XCTAssertNil(PairingOffer(["code": "0123-4567-89AB-CDEF-GHJK-MNPQ"], now: now))
        XCTAssertNil(PairingOffer(["id": "82a4d067d9aa39431ee15f1626c663e9"], now: now))
    }

    /// The camera passes over QR codes that aren't UrBudget's.
    func testOnlyUrBudgetsCodesAreScanned() {
        XCTAssertTrue(SyncKey.scannable("urbudget://pair/1/0123456789ABCDEFGHJKMNPQ"))
        XCTAssertTrue(SyncKey.scannable("URBUDGET://ASK/1/0123456789ABCDEFGHJKMNPQ"), "as a phone may hand a link on")
        XCTAssertTrue(SyncKey.isRequest("urbudget://ask/1/0123456789ABCDEFGHJKMNPQ"))
        XCTAssertTrue(SyncKey.isRequest("URBUDGET-ASK:1:0123456789ABCDEFGHJKMNPQ"))
        XCTAssertFalse(SyncKey.isRequest("urbudget://pair/1/0123456789ABCDEFGHJKMNPQ"))
        XCTAssertTrue(SyncKey.scannable("URBUDGET-PAIR:1:0123456789ABCDEFGHJKMNPQ"))
        XCTAssertTrue(SyncKey.scannable("URBUDGET-ASK:1:0123456789ABCDEFGHJKMNPQ"))
        XCTAssertTrue(SyncKey.scannable(" URBUDGET-NEXT:2:whatever a later version shows\n"), "the engine says to update")
        XCTAssertFalse(SyncKey.scannable("https://urbudget.app/"))
        XCTAssertFalse(SyncKey.scannable("0123-4567-89AB-CDEF-GHJK-MNPQ"), "typed, never scanned")
        XCTAssertFalse(SyncKey.scannable(""))
    }

    /// codes.rs `qr`: rows end to end, "1" for dark.
    func testQRSquaresReadRowByRow() throws {
        var rows = [String](repeating: String(repeating: "0", count: 21), count: 21)
        rows[0] = "1" + String(repeating: "0", count: 20)
        rows[2] = String(repeating: "0", count: 5) + "1" + String(repeating: "0", count: 15)
        rows[20] = String(repeating: "0", count: 20) + "1"
        let qr = try XCTUnwrap(QRModules(["size": 21, "modules": .string(rows.joined())]))
        XCTAssertEqual(qr.size, 21)
        XCTAssertTrue(qr.isDark(row: 0, column: 0))
        XCTAssertTrue(qr.isDark(row: 2, column: 5))
        XCTAssertFalse(qr.isDark(row: 5, column: 2), "rows first")
        XCTAssertTrue(qr.isDark(row: 20, column: 20))
        XCTAssertFalse(qr.isDark(row: 20, column: 19))
        XCTAssertFalse(qr.isDark(row: -1, column: 0))
        XCTAssertFalse(qr.isDark(row: 0, column: 21))

        // Anything that isn't a whole code draws nothing, and its characters can still be typed.
        XCTAssertNil(QRModules(["size": 21, "modules": .string(String(rows.joined().dropLast()))]))
        XCTAssertNil(QRModules(["size": 21, "modules": .string("2" + String(rows.joined().dropFirst()))]))
        XCTAssertNil(QRModules(["size": 3, "modules": "101010101"]))
        XCTAssertNil(QRModules(["size": 0, "modules": ""]))
        XCTAssertNil(QRModules(.null))
    }

    /// One pixel a square, black on white, inside the quiet zone.
    func testQRImageIsDarkOnWhiteWithAQuietZone() throws {
        var rows = [String](repeating: String(repeating: "0", count: 21), count: 21)
        rows[0] = "1" + String(repeating: "0", count: 20)
        rows[2] = String(repeating: "0", count: 5) + "1" + String(repeating: "0", count: 15)
        let image = try XCTUnwrap(try XCTUnwrap(QRModules(["size": 21, "modules": .string(rows.joined())])).image())
        XCTAssertEqual(image.width, 29)
        XCTAssertEqual(image.height, 29)
        let pixels = try gray(image, scale: 1)
        XCTAssertEqual(pixels.count, 29 * 29)
        XCTAssertEqual(pixels[4 * 29 + 4], 0, "the first square, after the quiet zone")
        XCTAssertEqual(pixels[6 * 29 + 9], 0, "row 2, column 5")
        XCTAssertEqual(pixels[9 * 29 + 6], 255)
        XCTAssertEqual(pixels.filter { $0 == 0 }.count, 2)
        XCTAssertEqual(pixels.filter { $0 == 255 }.count, 29 * 29 - 2)
    }

    /// A real QR code, taken apart into the engine's squares and drawn by the app, reads back as
    /// what it holds: nothing is mirrored, turned or cropped on the way to the screen.
    func testADrawnCodeReadsBack() throws {
        let payload = "URBUDGET-PAIR:1:0123456789ABCDEFGHJKMNPQ"
        let generator = try XCTUnwrap(CIFilter(name: "CIQRCodeGenerator"))
        generator.setValue(Data(payload.utf8), forKey: "inputMessage")
        generator.setValue("M", forKey: "inputCorrectionLevel")
        let output = try XCTUnwrap(generator.outputImage)
        let context = CIContext()
        let source = try XCTUnwrap(context.createCGImage(output, from: output.extent))
        let all = try gray(source, scale: 1)
        // The generator's own margin is left out: the engine sends the squares alone.
        var top = source.height, left = source.width, bottom = -1, right = -1
        for row in 0..<source.height {
            for column in 0..<source.width where all[row * source.width + column] < 128 {
                top = min(top, row)
                bottom = max(bottom, row)
                left = min(left, column)
                right = max(right, column)
            }
        }
        let size = bottom - top + 1
        XCTAssertEqual(right - left + 1, size)
        var modules = ""
        for row in 0..<size {
            for column in 0..<size { modules += all[(top + row) * source.width + left + column] < 128 ? "1" : "0" }
        }
        let qr = try XCTUnwrap(QRModules(["size": .int(Int64(size)), "modules": .string(modules)]))

        let scale = 8
        let drawn = try XCTUnwrap(qr.image())
        let side = drawn.width * scale
        let provider = try XCTUnwrap(CGDataProvider(data: Data(try gray(drawn, scale: scale)) as CFData))
        let enlarged = try XCTUnwrap(CGImage(width: side, height: side, bitsPerComponent: 8, bitsPerPixel: 8, bytesPerRow: side,
                                             space: CGColorSpaceCreateDeviceGray(), bitmapInfo: CGBitmapInfo(rawValue: CGImageAlphaInfo.none.rawValue),
                                             provider: provider, decode: nil, shouldInterpolate: false, intent: .defaultIntent))
        let detector = try XCTUnwrap(CIDetector(ofType: CIDetectorTypeQRCode, context: context, options: [CIDetectorAccuracy: CIDetectorAccuracyHigh]))
        let read = detector.features(in: CIImage(cgImage: enlarged)).compactMap { ($0 as? CIQRCodeFeature)?.messageString }
        XCTAssertEqual(read, [payload])
    }

    /// An image's pixels as 8-bit grey, top row first, each drawn `scale` pixels wide without smoothing.
    private func gray(_ image: CGImage, scale: Int) throws -> [UInt8] {
        let width = image.width * scale, height = image.height * scale
        var pixels = [UInt8](repeating: 0, count: width * height)
        try pixels.withUnsafeMutableBytes { buffer in
            let canvas = try XCTUnwrap(CGContext(data: buffer.baseAddress, width: width, height: height, bitsPerComponent: 8, bytesPerRow: width,
                                                 space: CGColorSpaceCreateDeviceGray(), bitmapInfo: CGImageAlphaInfo.none.rawValue))
            canvas.interpolationQuality = .none
            canvas.draw(image, in: CGRect(x: 0, y: 0, width: width, height: height))
        }
        return pixels
    }
}
