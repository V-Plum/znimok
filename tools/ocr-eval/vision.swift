// Apple Vision over the OCR evaluation set (ZK-120): the reference on-device engine that
// already reads Ukrainian. `swift tools/ocr-eval/vision.swift <set dir> <out.json>`.
import Foundation
import Vision

let args = CommandLine.arguments
let dir = URL(fileURLWithPath: args[1])
var out: [String: String] = [:]
let files = try FileManager.default.contentsOfDirectory(at: dir, includingPropertiesForKeys: nil)
    .filter { $0.pathExtension == "png" }.sorted { $0.lastPathComponent < $1.lastPathComponent }
for f in files {
    let req = VNRecognizeTextRequest()
    req.recognitionLevel = .accurate
    req.usesLanguageCorrection = true
    req.recognitionLanguages = ["uk-UA", "en-US"]
    try VNImageRequestHandler(url: f).perform([req])
    let lines = (req.results ?? [])
        .sorted { $0.boundingBox.minY > $1.boundingBox.minY }
        .compactMap { $0.topCandidates(1).first?.string }
    out[f.lastPathComponent] = lines.joined(separator: "\n")
}
let data = try JSONSerialization.data(withJSONObject: out, options: [.prettyPrinted])
try data.write(to: URL(fileURLWithPath: args[2]))
print("vision: \(out.count) pictures")
