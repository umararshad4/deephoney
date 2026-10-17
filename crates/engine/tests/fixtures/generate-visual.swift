import Foundation
import CoreGraphics
import CoreText
import ImageIO
import UniformTypeIdentifiers
import PDFKit
let w = 1000, h = 1200
let ctx = CGContext(data:nil,width:w,height:h,bitsPerComponent:8,bytesPerRow:w*4,space:CGColorSpaceCreateDeviceRGB(),bitmapInfo:CGImageAlphaInfo.premultipliedLast.rawValue)!
ctx.setFillColor(CGColor(gray:1,alpha:1));ctx.fill(CGRect(x:0,y:0,width:w,height:h))
func text(_ s:String,_ x:Double,_ y:Double,_ size:Double=32) {
 let attrs:[NSAttributedString.Key:Any] = [NSAttributedString.Key(kCTFontAttributeName as String): CTFontCreateWithName("Helvetica" as CFString,size,nil), NSAttributedString.Key(kCTForegroundColorAttributeName as String): CGColor(gray:0,alpha:1)]
 let line=CTLineCreateWithAttributedString(NSAttributedString(string:s,attributes:attrs));ctx.textPosition=CGPoint(x:x,y:y);CTLineDraw(line,ctx)
}
text("Meridian audited sales",80,1100,40)
text("Year",100,990);text("Revenue",450,990)
text("2024",100,920);text("125",450,920)
text("2025",100,850);text("175",450,850)
ctx.setStrokeColor(CGColor(gray:0,alpha:1));ctx.setLineWidth(2)
for y in [820.0,890,960,1030] {ctx.move(to:CGPoint(x:80,y:y));ctx.addLine(to:CGPoint(x:800,y:y));ctx.strokePath()}
text("Revenue by year (USD millions)",80,720,34)
ctx.setFillColor(CGColor(red:0.2,green:0.35,blue:0.65,alpha:1));ctx.fill(CGRect(x:180,y:250,width:180,height:250));ctx.fill(CGRect(x:540,y:250,width:180,height:350))
text("125",230,530);text("175",590,630);text("2024",225,195);text("2025",585,195)
let image = ctx.makeImage()!
let url=URL(fileURLWithPath:CommandLine.arguments[1])
let pdf=CGContext(url as CFURL,mediaBox:nil,nil)!
pdf.beginPDFPage([kCGPDFContextMediaBox as String:CGRect(x:0,y:0,width:w,height:h)] as CFDictionary)
pdf.draw(image,in:CGRect(x:0,y:0,width:w,height:h));pdf.endPDFPage();pdf.closePDF()

// The rotation is PDF page metadata, as produced by document scanners.
let rotated = PDFDocument(url: url)!
rotated.page(at: 0)!.rotation = 90
rotated.write(to: url.deletingLastPathComponent().appendingPathComponent("scanned-rotated.pdf"))
let mixedURL = url.deletingLastPathComponent().appendingPathComponent("mixed-text-scan.pdf")
let mixed = CGContext(mixedURL as CFURL, mediaBox: nil, nil)!
mixed.beginPDFPage([kCGPDFContextMediaBox as String:CGRect(x:0,y:0,width:w,height:h)] as CFDictionary)
let words = NSAttributedString(string:"This page has selectable source text. Meridian annual report includes a scanned table on page two.", attributes:[NSAttributedString.Key(kCTFontAttributeName as String):CTFontCreateWithName("Helvetica" as CFString,24,nil)])
mixed.textPosition = CGPoint(x:70,y:1000);CTLineDraw(CTLineCreateWithAttributedString(words),mixed)
mixed.endPDFPage()
mixed.beginPDFPage([kCGPDFContextMediaBox as String:CGRect(x:0,y:0,width:w,height:h)] as CFDictionary)
mixed.draw(image,in:CGRect(x:0,y:0,width:w,height:h));mixed.endPDFPage();mixed.closePDF()
