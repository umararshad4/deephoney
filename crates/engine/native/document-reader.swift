// Built once by Cargo. Only a disposable, bounded child handles untrusted PDF data.
import Foundation
import CoreGraphics
import Vision
import NaturalLanguage
import CoreML
import ImageIO
import UniformTypeIdentifiers
import Darwin

struct Line: Codable {
    let text: String
    let confidence: Float
    // Normalized image coordinates: origin at bottom left, width/height in [0,1].
    let box: [Double]
}
struct Page: Codable {
    let page: Int
    let lines: [Line]
    let png: String?
    let warnings: [String]
}
struct Output: Codable {
    let pages: [Page]
    let totalPages: Int
    let error: String?
}

func recognize(_ image: CGImage) throws -> [Line] {
    let request = VNRecognizeTextRequest()
    request.revision = VNRecognizeTextRequestRevision3
    // Keep work in the bounded child instead of persistent ANE compilation/services.
    for (stage, devices) in try request.supportedComputeStageDevices {
        guard let cpu = devices.first(where: { if case .cpu = $0 { return true }; return false }) else {
            throw NSError(domain: "CPU OCR compute device unavailable", code: 1)
        }
        request.setComputeDevice(cpu, for: stage)
    }
    request.recognitionLevel = .fast // Predictable CPU path; avoids unbounded accurate-model cold-start compilation.
    request.usesLanguageCorrection = false // Preserve numbers and avoid dictionary substitutions.
    request.automaticallyDetectsLanguage = false
    request.recognitionLanguages = ["en-US"]
    try VNImageRequestHandler(cgImage: image).perform([request])
    return (request.results ?? []).prefix(2000).compactMap { observation -> Line? in
        guard let text = observation.topCandidates(1).first else { return nil }
        let b = observation.boundingBox
        return Line(text: String(text.string.prefix(2000)), confidence: text.confidence,
                    box: [b.minX, b.minY, b.width, b.height])
    }.sorted {
        let leftRow = Int(($0.box[1] / 0.008).rounded())
        let rightRow = Int(($1.box[1] / 0.008).rounded())
        return leftRow != rightRow ? leftRow > rightRow : $0.box[0] < $1.box[0]
    }
}
func rotate(_ image: CGImage) throws -> CGImage {
    guard let context = CGContext(data: nil, width: image.height, height: image.width, bitsPerComponent: 8,
                                  bytesPerRow: image.height * 4, space: CGColorSpaceCreateDeviceRGB(),
                                  bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue) else {
        throw NSError(domain: "cannot rotate OCR image", code: 1)
    }
    context.translateBy(x: CGFloat(image.height), y: 0)
    context.rotate(by: .pi / 2)
    context.draw(image, in: CGRect(x: 0, y: 0, width: image.width, height: image.height))
    guard let output = context.makeImage() else { throw NSError(domain: "cannot rotate OCR image", code: 1) }
    return output
}
func run() throws -> Output {
    var cpu = rlimit(rlim_cur: 45, rlim_max: 45)
    guard setrlimit(RLIMIT_CPU, &cpu) == 0 else { throw NSError(domain: "CPU limit unavailable", code: 1) }
    let data = FileHandle.standardInput.readData(ofLength: 12 * 1024 * 1024 + 1)
    guard data.count <= 12 * 1024 * 1024,
          let provider = CGDataProvider(data: data as CFData),
          let doc = CGPDFDocument(provider), !doc.isEncrypted, doc.numberOfPages > 0 else {
        throw NSError(domain: "invalid, encrypted or oversized PDF", code: 1)
    }
    let visual = CommandLine.arguments.contains("--visual")
    var output: [Page] = []
    for number in 1...min(doc.numberOfPages, 12) {
        try autoreleasepool {
            guard let page = doc.page(at: number) else { return }
            let rect = page.getBoxRect(.mediaBox)
            guard rect.width.isFinite, rect.height.isFinite, rect.width > 0, rect.height > 0 else {
                throw NSError(domain: "invalid PDF page geometry", code: 1)
            }
            let scale = min(2.5, 1800 / max(rect.width, rect.height))
            let swapped = abs(page.rotationAngle) % 180 == 90
            let width = max(1, Int((swapped ? rect.height : rect.width) * scale))
            let height = max(1, Int((swapped ? rect.width : rect.height) * scale))
            guard let context = CGContext(data: nil, width: width, height: height, bitsPerComponent: 8,
                                          bytesPerRow: width * 4, space: CGColorSpaceCreateDeviceRGB(),
                                          bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue) else {
                throw NSError(domain: "cannot render PDF page", code: 1)
            }
            context.setFillColor(CGColor(gray: 1, alpha: 1))
            context.fill(CGRect(x: 0, y: 0, width: width, height: height))
            context.concatenate(page.getDrawingTransform(.mediaBox, rect: CGRect(x: 0, y: 0, width: width, height: height), rotate: 0, preserveAspectRatio: true))
            context.drawPDFPage(page)
            guard let image = context.makeImage() else { throw NSError(domain: "cannot create page image", code: 1) }
            var selected = image
            var candidate = image
            var lines: [Line] = []
            var score = -1.0
            // Scanners may rotate pixels, page metadata, or both. Retain the orientation with
            // strongest English text signal, and return boxes in that same rendered image.
            // Raw character count alone can prefer slightly longer upside-down gibberish.
            // The OCR model is English-only; numbers/short labels retain a count baseline.
            for _ in 0..<4 {
                let recognized = try recognize(candidate)
                let count = recognized.reduce(0) { $0 + $1.text.count }
                let language = NLLanguageRecognizer()
                language.processString(recognized.map { $0.text }.joined(separator: " "))
                let english = language.languageHypotheses(withMaximum: 8)[.english] ?? 0
                // Weak language guesses on degraded scans must not outrank the count baseline.
                let candidateScore = Double(count) * (english >= 0.8 ? 1 + english : 1)
                if candidateScore > score { score = candidateScore; lines = recognized; selected = candidate }
                candidate = try rotate(candidate)
            }
            var encoded: String? = nil
            var warnings: [String] = []
            if visual {
                let bytes = NSMutableData()
                if let destination = CGImageDestinationCreateWithData(bytes, UTType.png.identifier as CFString, 1, nil) {
                    CGImageDestinationAddImage(destination, selected, nil)
                    if CGImageDestinationFinalize(destination) && bytes.length <= 2 * 1024 * 1024 {
                        encoded = (bytes as Data).base64EncodedString()
                    } else {
                        warnings.append("Visual image unavailable: PNG encoding failed or exceeded the 2 MiB image limit; local OCR retained.")
                    }
                } else {
                    warnings.append("Visual image unavailable: cannot create PNG encoder; local OCR retained.")
                }
            }
            if lines.count >= 2000 { warnings.append("OCR line limit reached; later text regions may be omitted.") }
            if lines.contains(where: { $0.text.count >= 2000 }) { warnings.append("OCR line text limit reached; long regions may be truncated.") }
            if lines.isEmpty { warnings.append("Local OCR found no readable text on this page.") }
            output.append(Page(page: number, lines: lines, png: encoded, warnings: warnings))
        }
    }
    return Output(pages: output, totalPages: doc.numberOfPages, error: nil)
}
let result: Output
do { result = try run() }
catch { result = Output(pages: [], totalPages: 0, error: String(describing: error)) }
if let data = try? JSONEncoder().encode(result) {
    if data.count <= 36 * 1024 * 1024 { FileHandle.standardOutput.write(data) }
    else if let failure = try? JSONEncoder().encode(Output(pages: [], totalPages: 0, error: "native OCR exceeded 36 MiB output limit")) {
        FileHandle.standardOutput.write(failure)
    }
}
