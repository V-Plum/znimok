// Finder thumbnails of .znimok files (ZK-76): a Quick Look thumbnail extension over the Rust
// core. Reads only the head of the file until the stored thumbnail (a PNG the app renders when it
// saves) appears — never the document's pixels — and draws it scaled into Finder's box.

import CoreGraphics
import Foundation
import ImageIO
import QuickLookThumbnailing

@_silgen_name("znimok_thumbnail_png")
func znimok_thumbnail_png(_ data: UnsafePointer<UInt8>, _ len: Int, _ outLen: UnsafeMutablePointer<Int>) -> UnsafeMutablePointer<UInt8>?

@_silgen_name("znimok_thumbnail_free")
func znimok_thumbnail_free(_ p: UnsafeMutablePointer<UInt8>, _ len: Int)

enum ThumbnailError: Error { case noThumbnail }

/// The stored thumbnail as a CGImage, reading the file in 1 MB steps (at most 64 MB).
func storedThumbnail(_ url: URL) -> CGImage? {
    guard let fh = try? FileHandle(forReadingFrom: url) else { return nil }
    defer { try? fh.close() }
    var head = Data()
    while head.count < 64 << 20 {
        guard let chunk = try? fh.read(upToCount: 1 << 20), !chunk.isEmpty else { break }
        head.append(chunk)
        var len = 0
        let png: Data? = head.withUnsafeBytes { raw in
            guard let base = raw.bindMemory(to: UInt8.self).baseAddress,
                  let p = znimok_thumbnail_png(base, raw.count, &len) else { return nil }
            defer { znimok_thumbnail_free(p, len) }
            return Data(bytes: p, count: len)
        }
        if let png, let src = CGImageSourceCreateWithData(png as CFData, nil) {
            return CGImageSourceCreateImageAtIndex(src, 0, nil)
        }
    }
    return nil
}

class ThumbnailProvider: QLThumbnailProvider {
    override func provideThumbnail(for request: QLFileThumbnailRequest,
                                   _ handler: @escaping (QLThumbnailReply?, Error?) -> Void) {
        guard let image = storedThumbnail(request.fileURL) else {
            handler(nil, ThumbnailError.noThumbnail)
            return
        }
        let w = CGFloat(image.width), h = CGFloat(image.height)
        let scale = min(request.maximumSize.width / w, request.maximumSize.height / h, 1)
        let size = CGSize(width: max(1, (w * scale).rounded()), height: max(1, (h * scale).rounded()))
        handler(QLThumbnailReply(contextSize: size, currentContextDrawing: { () -> Bool in
            guard let ctx = NSGraphicsContextCurrentCG() else { return false }
            ctx.interpolationQuality = .high
            ctx.draw(image, in: CGRect(origin: .zero, size: size))
            return true
        }), nil)
    }
}

/// The current CoreGraphics context without importing AppKit into the extension.
func NSGraphicsContextCurrentCG() -> CGContext? {
    guard let cls = NSClassFromString("NSGraphicsContext") as? NSObject.Type,
          let current = cls.value(forKey: "currentContext") as? NSObject else { return nil }
    return current.value(forKey: "CGContext") as! CGContext?
}
