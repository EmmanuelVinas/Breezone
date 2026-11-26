// Generates the iOS icon of debug builds: the store icon with a "BETA" band.
// Run from mobile/ after changing the store icon: swift scripts/gen-debug-app-icon.swift
import AppKit

let assets = "iosApp/iosApp/Assets.xcassets"
let source = "\(assets)/AppIcon.appiconset/app-icon-1024.png"
let target = "\(assets)/AppIcon-Debug.appiconset"

guard let icon = NSImage(contentsOfFile: source) else {
    FileHandle.standardError.write("Cannot read \(source)\n".data(using: .utf8)!)
    exit(1)
}

let size = 1024
// No alpha channel: App Store icons must be opaque.
let context = CGContext(
    data: nil, width: size, height: size, bitsPerComponent: 8, bytesPerRow: 0,
    space: CGColorSpace(name: CGColorSpace.sRGB)!, bitmapInfo: CGImageAlphaInfo.noneSkipLast.rawValue
)!
NSGraphicsContext.saveGraphicsState()
NSGraphicsContext.current = NSGraphicsContext(cgContext: context, flipped: false)

let full = NSRect(x: 0, y: 0, width: size, height: size)
icon.draw(in: full)

// Same colors as the Android debug icon: the mark's navy and the app's warning yellow.
let band = NSRect(x: 0, y: 74, width: size, height: 136)
NSColor(srgbRed: 0x0F / 255.0, green: 0x1B / 255.0, blue: 0x2D / 255.0, alpha: 1).setFill()
band.fill()

let style = NSMutableParagraphStyle()
style.alignment = .center
let text = NSAttributedString(string: "BETA", attributes: [
    .font: NSFont.systemFont(ofSize: 96, weight: .heavy),
    .foregroundColor: NSColor(srgbRed: 0xF2 / 255.0, green: 0xC1 / 255.0, blue: 0x4E / 255.0, alpha: 1),
    .kern: 18,
    .paragraphStyle: style,
])
let textHeight = text.size().height
text.draw(in: NSRect(x: 0, y: band.midY - textHeight / 2, width: CGFloat(size), height: textHeight))

NSGraphicsContext.restoreGraphicsState()

try! FileManager.default.createDirectory(atPath: target, withIntermediateDirectories: true)
let bitmap = NSBitmapImageRep(cgImage: context.makeImage()!)
try! bitmap.representation(using: .png, properties: [:])!.write(to: URL(fileURLWithPath: "\(target)/app-icon-1024.png"))
try! """
{
  "images" : [
    {
      "filename" : "app-icon-1024.png",
      "idiom" : "universal",
      "platform" : "ios",
      "size" : "1024x1024"
    }
  ],
  "info" : {
    "author" : "xcode",
    "version" : 1
  }
}

""".write(toFile: "\(target)/Contents.json", atomically: true, encoding: .utf8)
print("Wrote \(target)")
