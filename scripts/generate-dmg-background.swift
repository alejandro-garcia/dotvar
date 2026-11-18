import AppKit

guard CommandLine.arguments.count == 3,
      let logo = NSImage(contentsOfFile: CommandLine.arguments[1]) else {
    fatalError("Usage: swift generate-dmg-background.swift logo.svg background.tiff")
}

func color(_ hex: UInt32, alpha: CGFloat = 1) -> NSColor {
    NSColor(srgbRed: CGFloat((hex >> 16) & 255) / 255,
            green: CGFloat((hex >> 8) & 255) / 255,
            blue: CGFloat(hex & 255) / 255, alpha: alpha)
}

// Match the Finder layout in dmg-layout.py. All positions are in points.
let size = NSSize(width: 720, height: 440)
let image = NSImage(size: size)
let panel = NSRect(x: 24, y: 24, width: 672, height: 392)
let outline = NSBezierPath(roundedRect: panel, xRadius: 16, yRadius: 16)
let iconCenterY = size.height - 213

for scale in [1, 2] {
    let bitmap = NSBitmapImageRep(
        bitmapDataPlanes: nil, pixelsWide: Int(size.width) * scale,
        pixelsHigh: Int(size.height) * scale, bitsPerSample: 8,
        samplesPerPixel: 4, hasAlpha: true, isPlanar: false,
        colorSpaceName: .calibratedRGB, bytesPerRow: 0, bitsPerPixel: 0
    )!
    bitmap.size = size
    NSGraphicsContext.saveGraphicsState()
    NSGraphicsContext.current = NSGraphicsContext(bitmapImageRep: bitmap)

    color(0xfafaf9).setFill()
    NSRect(origin: .zero, size: size).fill()
    NSGradient(starting: color(0xffffff), ending: color(0xf5f5f4))!.draw(in: outline, angle: -90)
    color(0xe7e5e4).setStroke()
    outline.lineWidth = 1
    outline.stroke()

    let logoSize = NSSize(width: 155, height: 155 * logo.size.height / logo.size.width)
    logo.draw(in: NSRect(x: (size.width - logoSize.width) / 2, y: 342,
                         width: logoSize.width, height: logoSize.height))

    // A quiet editor grid behind the native drag targets.
    color(0x79716b, alpha: 0.15).setFill()
    for y in stride(from: 144, through: 304, by: 16) {
        for x in stride(from: 64, through: 656, by: 16) {
            NSBezierPath(ovalIn: NSRect(x: CGFloat(x), y: CGFloat(y), width: 1.2, height: 1.2)).fill()
        }
    }

    // The orange syntax accent grows stronger toward the Applications shortcut.
    for (index, opacity) in [0.36, 0.60, 0.94].enumerated() {
        let x = CGFloat(332 + index * 24)
        let chevron = NSBezierPath()
        chevron.move(to: NSPoint(x: x, y: iconCenterY + 9))
        chevron.line(to: NSPoint(x: x + 8, y: iconCenterY))
        chevron.line(to: NSPoint(x: x, y: iconCenterY - 9))
        chevron.lineWidth = 3.5
        chevron.lineCapStyle = .round
        chevron.lineJoinStyle = .round
        color(0xbb4d00, alpha: opacity).setStroke()
        chevron.stroke()
    }

    let instruction = NSAttributedString(string: "Drag Dotvar into Applications", attributes: [
        .font: NSFont.systemFont(ofSize: 15, weight: .medium),
        .foregroundColor: color(0x57534d),
    ])
    instruction.draw(at: NSPoint(x: (size.width - instruction.size().width) / 2, y: 75))

    NSGraphicsContext.restoreGraphicsState()
    image.addRepresentation(bitmap)
}

try image.tiffRepresentation!.write(to: URL(fileURLWithPath: CommandLine.arguments[2]))
