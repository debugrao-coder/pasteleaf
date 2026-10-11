import Foundation
import ImageIO
import UniformTypeIdentifiers

/// Inspect headers before allocating full raster memory. Capture encoding runs off the UI thread.
enum SafeImage {
    static let byteLimit = 100 * 1024 * 1024
    static let pixelLimit = 16 * 1024 * 1024
    static let dimensionLimit = 16_384
    static let decodedByteLimit = 128 * 1024 * 1024
    static func validateDimensions(width: Int, height: Int, bitsPerComponent: Int = 8) throws {
        guard width > 0, height > 0, width <= dimensionLimit, height <= dimensionLimit,
              width <= pixelLimit / height, (1...32).contains(bitsPerComponent) else { throw sizeError }
        // Conservatively budget RGBA pixels before ImageIO allocates a raster. This is a
        // decoded-buffer ceiling, not a claim about total codec/process memory usage.
        let bytesPerPixel = 4 * ((bitsPerComponent + 7) / 8)
        guard width * height <= decodedByteLimit / bytesPerPixel else { throw sizeError }
    }
    static func validate(_ data: Data) throws {
        guard data.count <= byteLimit, let source = CGImageSourceCreateWithData(data as CFData, [kCGImageSourceShouldCache: false] as CFDictionary),
              let properties = CGImageSourceCopyPropertiesAtIndex(source, 0, nil) as? [CFString: Any],
              let width = properties[kCGImagePropertyPixelWidth] as? Int, let height = properties[kCGImagePropertyPixelHeight] as? Int else { throw sizeError }
        try validateDimensions(width: width, height: height, bitsPerComponent: properties[kCGImagePropertyDepth] as? Int ?? 8)
    }
    static func previewSource(at url: URL) throws -> CGImageSource {
        let metadata = try url.resourceValues(forKeys: [.fileSizeKey, .isRegularFileKey, .isSymbolicLinkKey])
        guard metadata.isRegularFile == true, metadata.isSymbolicLink != true,
              let size = metadata.fileSize, size <= byteLimit else { throw sizeError }
        let data = try Data(contentsOf: url, options: [.mappedIfSafe])
        try validate(data)
        guard let source = CGImageSourceCreateWithData(data as CFData, [kCGImageSourceShouldCache: false] as CFDictionary) else { throw sizeError }
        return source
    }
    private static var sizeError: CoreError {
        CoreError(code: "image_size", message: "图片无法安全解码或超过上限（100 MiB、16,777,216 像素、单边 16384、解码缓冲区 128 MiB）。未处理这张图片。")
    }
    static func pasteboardType(_ data: Data) throws -> String {
        try validate(data)
        guard let source = CGImageSourceCreateWithData(data as CFData, [kCGImageSourceShouldCache: false] as CFDictionary), let type = CGImageSourceGetType(source) as String?, [UTType.png.identifier, UTType.jpeg.identifier].contains(type) else {
            throw CoreError(code: "image_format", message: "不支持的图片粘贴格式，未改写剪贴板。")
        }
        return type
    }
    static func pngData(_ data: Data) throws -> Data {
        try validate(data)
        guard let source = CGImageSourceCreateWithData(data as CFData, [kCGImageSourceShouldCache: false] as CFDictionary), let image = CGImageSourceCreateImageAtIndex(source, 0, [kCGImageSourceShouldCache: false] as CFDictionary) else {
            throw CoreError(code: "image_decode", message: "无法解码图片，未写入历史。")
        }
        guard image.bytesPerRow > 0, image.height <= decodedByteLimit / image.bytesPerRow else { throw sizeError }
        let output = NSMutableData()
        guard let destination = CGImageDestinationCreateWithData(output, UTType.png.identifier as CFString, 1, nil) else { throw CoreError(code: "image_encode", message: "无法转换图片格式。") }
        CGImageDestinationAddImage(destination, image, nil)
        guard CGImageDestinationFinalize(destination), output.length <= byteLimit else { throw CoreError(code: "image_size", message: "转换后的图片超过 100 MiB，未写入历史。") }
        return output as Data
    }
}
