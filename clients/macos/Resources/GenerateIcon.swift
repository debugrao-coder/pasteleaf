import AppKit

/// Packages the shared brand artwork; never redraws or recolors the master.
/// Explicit bitmap dimensions avoid Retina-dependent lockFocus output sizes.
@main
struct GenerateIcon {
    static func main() throws {
        let args = CommandLine.arguments
        guard args.count == 3 || args.count == 4 else {
            throw failure("Usage: generate-icon MASTER.png OUTPUT.iconset [OUTPUT.ico]")
        }
        let source = URL(fileURLWithPath: args[1])
        let data = try Data(contentsOf: source)
        guard let bitmap = NSBitmapImageRep(data: data), bitmap.pixelsWide == bitmap.pixelsHigh,
              bitmap.pixelsWide >= 1024, bitmap.hasAlpha, let image = NSImage(data: data) else {
            throw failure("Expected a square, alpha-preserving PNG master of at least 1024 pixels")
        }
        let output = URL(fileURLWithPath: args[2], isDirectory: true)
        try FileManager.default.createDirectory(at: output, withIntermediateDirectories: true)
        var frames: [Int: Data] = [:]
        for pixels in [16, 24, 32, 48, 64, 128, 256, 512, 1024] {
            frames[pixels] = try png(image: image, pixels: pixels)
        }
        for base in [16, 32, 128, 256, 512] {
            for scale in [1, 2] {
                let name = "icon_\(base)x\(base)" + (scale == 2 ? "@2x" : "") + ".png"
                try frames[base * scale]!.write(to: output.appendingPathComponent(name), options: .atomic)
            }
        }
        if args.count == 4 {
            try ico(frames: frames).write(to: URL(fileURLWithPath: args[3]), options: .atomic)
        }
    }

    static func png(image: NSImage, pixels: Int) throws -> Data {
        guard let bitmap = NSBitmapImageRep(bitmapDataPlanes: nil, pixelsWide: pixels, pixelsHigh: pixels,
                bitsPerSample: 8, samplesPerPixel: 4, hasAlpha: true, isPlanar: false,
                colorSpaceName: .deviceRGB, bytesPerRow: pixels * 4, bitsPerPixel: 32),
              let context = NSGraphicsContext(bitmapImageRep: bitmap) else {
            throw failure("Could not allocate icon bitmap")
        }
        NSGraphicsContext.saveGraphicsState()
        defer { NSGraphicsContext.restoreGraphicsState() }
        NSGraphicsContext.current = context
        context.imageInterpolation = .high
        image.draw(in: NSRect(x: 0, y: 0, width: pixels, height: pixels), from: .zero, operation: .copy, fraction: 1)
        guard let png = bitmap.representation(using: .png, properties: [:]) else {
            throw failure("Could not encode icon bitmap")
        }
        return png
    }

    /// Windows 10/11 support PNG-compressed 32-bit ICO entries.
    static func ico(frames: [Int: Data]) -> Data {
        let sizes = [16, 24, 32, 48, 64, 128, 256]
        var directory = Data()
        func u16(_ n: Int) { directory.append(contentsOf: [UInt8(n & 255), UInt8((n >> 8) & 255)]) }
        func u32(_ n: Int) { for shift in stride(from: 0, to: 32, by: 8) { directory.append(UInt8((n >> shift) & 255)) } }
        u16(0); u16(1); u16(sizes.count)
        var offset = 6 + sizes.count * 16
        for size in sizes {
            let frame = frames[size]!
            directory.append(contentsOf: [UInt8(size % 256), UInt8(size % 256), 0, 0])
            u16(1); u16(32); u32(frame.count); u32(offset)
            offset += frame.count
        }
        for size in sizes { directory.append(frames[size]!) }
        return directory
    }

    static func failure(_ message: String) -> NSError {
        NSError(domain: "PasteLeaf.Icon", code: 1, userInfo: [NSLocalizedDescriptionKey: message])
    }
}
