// Writes a QR code for a link as an SVG, for the site's "scan with your phone" codes.
//
//   swift scripts/qr.swift <url> <out.svg>        (from web/, on a Mac)
//
// CoreImage makes the code (error correction M); this script draws its modules as one path,
// black on white with a four-module quiet zone, then reads the code back with CoreImage's own
// QR detector and stops unless it says exactly <url>. Run `npm run images` afterwards so pages
// link the new file by its hash.

import CoreImage
import Foundation

let args = CommandLine.arguments
guard args.count == 3 else {
    FileHandle.standardError.write("usage: swift scripts/qr.swift <url> <out.svg>\n".data(using: .utf8)!)
    exit(2)
}
let url = args[1], out = args[2]

let filter = CIFilter(name: "CIQRCodeGenerator")!
filter.setValue(Data(url.utf8), forKey: "inputMessage")
filter.setValue("M", forKey: "inputCorrectionLevel")
let code = filter.outputImage!

// One pixel a module. CoreImage's image already has a one-module margin; it's cut off here and
// the quiet zone is added below.
let context = CIContext()
let size = Int(code.extent.width)
var pixels = [UInt8](repeating: 0, count: size * size * 4)
context.render(code, toBitmap: &pixels, rowBytes: size * 4, bounds: code.extent, format: .RGBA8,
               colorSpace: CGColorSpaceCreateDeviceRGB())
let modules = (1..<size - 1).map { y in (1..<size - 1).map { x in pixels[(y * size + x) * 4] < 128 } }
let n = modules.count

// The finder patterns must be top left, top right and bottom left, or the bitmap came out flipped.
func finder(_ x: Int, _ y: Int) -> Bool { (0..<7).allSatisfy { modules[y][x + $0] && modules[y + 6][x + $0] } }
guard finder(0, 0), finder(n - 7, 0), finder(0, n - 7), !finder(n - 7, n - 7) else {
    FileHandle.standardError.write("qr: the code's finder patterns aren't where they belong\n".data(using: .utf8)!)
    exit(1)
}

let quiet = 4
let side = n + 2 * quiet
var path = ""
for (y, row) in modules.enumerated() {
    var x = 0
    while x < n {
        guard row[x] else { x += 1; continue }
        var run = 1
        while x + run < n && row[x + run] { run += 1 }
        path += "M\(x + quiet) \(y + quiet)h\(run)v1h-\(run)z"
        x += run
    }
}
let svg = """
<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 \(side) \(side)" shape-rendering="crispEdges">\
<rect width="\(side)" height="\(side)" fill="#fff"/><path d="\(path)" fill="#000"/></svg>

"""

// Read it back: draw the modules ten pixels each and ask CoreImage's detector what it says.
let scale = 10
var bitmap = [UInt8](repeating: 255, count: side * scale * side * scale)
for (y, row) in modules.enumerated() {
    for (x, dark) in row.enumerated() where dark {
        for dy in 0..<scale {
            for dx in 0..<scale { bitmap[((y + quiet) * scale + dy) * side * scale + (x + quiet) * scale + dx] = 0 }
        }
    }
}
let drawn = CIImage(bitmapData: Data(bitmap), bytesPerRow: side * scale, size: CGSize(width: side * scale, height: side * scale),
                    format: .L8, colorSpace: CGColorSpaceCreateDeviceGray())
let detector = CIDetector(ofType: CIDetectorTypeQRCode, context: context, options: [CIDetectorAccuracy: CIDetectorAccuracyHigh])!
let read = (detector.features(in: drawn).first as? CIQRCodeFeature)?.messageString
guard read == url else {
    FileHandle.standardError.write("qr: the code reads back as \(read ?? "nothing"), not \(url)\n".data(using: .utf8)!)
    exit(1)
}
try! svg.write(toFile: out, atomically: true, encoding: .utf8)
print("\(out): \(n)×\(n) modules, reads back as \(url)")
