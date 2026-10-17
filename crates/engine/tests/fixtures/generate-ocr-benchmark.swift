// Raster-only synthetic pages generated from checked-in truth; no external content.
import Foundation
import CoreGraphics
import CoreText
import PDFKit
let spec = try JSONSerialization.jsonObject(with: Data(contentsOf: URL(fileURLWithPath: CommandLine.arguments[1]))) as! [String: Any]
let output = URL(fileURLWithPath: CommandLine.arguments[2], isDirectory: true)
for item in spec["cases"] as! [[String: Any]] {
    let lines = (item["lines"] ?? spec["lines"]) as! [String]
    let scale = item["scale"] as! Double
    let width = Int(1000 * scale), height = Int(1200 * scale)
    let context = CGContext(data: nil, width: width, height: height, bitsPerComponent: 8, bytesPerRow: width * 4,
                            space: CGColorSpaceCreateDeviceRGB(), bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue)!
    context.setFillColor(CGColor(gray: 1, alpha: 1)); context.fill(CGRect(x: 0, y: 0, width: width, height: height))
    context.scaleBy(x: scale, y: scale)
    let attributes: [NSAttributedString.Key: Any] = [
        NSAttributedString.Key(kCTFontAttributeName as String): CTFontCreateWithName("Helvetica" as CFString, item["font_size"] as! Double, nil),
        NSAttributedString.Key(kCTForegroundColorAttributeName as String): CGColor(gray: 1 - (item["contrast"] as! Double), alpha: 1)]
    for (index, text) in lines.enumerated() {
        context.textPosition = CGPoint(x: 70, y: 1100 - index * 85)
        CTLineDraw(CTLineCreateWithAttributedString(NSAttributedString(string: text, attributes: attributes)), context)
    }
    let url = output.appendingPathComponent((item["id"] as! String) + ".pdf")
    var mediaBox = CGRect(x: 0, y: 0, width: 1000, height: 1200)
    let pdf = CGContext(url as CFURL, mediaBox: &mediaBox, nil)!
    pdf.beginPDFPage(nil)
    pdf.draw(context.makeImage()!, in: CGRect(x: 0, y: 0, width: 1000, height: 1200))
    pdf.endPDFPage(); pdf.closePDF()
    // Reject invalid dictionary bridging/default Letter geometry instead of scoring a different corpus.
    guard let generated = PDFDocument(url: url),
          generated.page(at: 0)?.bounds(for: .mediaBox).size == mediaBox.size else {
        fatalError("generated fixture has incorrect page geometry")
    }
    if item["rotation"] as! Int != 0 {
        let document = PDFDocument(url: url)!
        document.page(at: 0)!.rotation = item["rotation"] as! Int
        guard document.write(to: url) else { fatalError("cannot write rotated fixture") }
    }
}
