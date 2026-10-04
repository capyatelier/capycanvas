import Foundation

func savedRulers(_ url: URL) throws -> JSON {
    let bytes = try Data(contentsOf: url)
    func word(_ offset: Int, _ count: Int) throws -> Int {
        guard offset >= 0, count <= bytes.count - offset else { throw HostFailure(message: "Incomplete package header") }
        return bytes[offset..<(offset + count)].enumerated().reduce(0) { $0 | Int($1.element) << ($1.offset * 8) }
    }
    var offset = 0
    while try word(offset, 4) == 0x04034b50 {
        let method = try word(offset + 8, 2), length = try word(offset + 18, 4), decoded = try word(offset + 22, 4)
        let nameLength = try word(offset + 26, 2), extra = try word(offset + 28, 2)
        let start = offset + 30 + nameLength + extra
        guard start <= bytes.count, length <= bytes.count - start else { throw HostFailure(message: "Incomplete package member") }
        let name = String(decoding: bytes[(offset + 30)..<(offset + 30 + nameLength)], as: UTF8.self)
        if name == "manifest.json" {
            guard decoded <= 1024 * 1024, method == 0 || method == 8 else { throw HostFailure(message: "Unexpected package manifest") }
            let encoded = bytes.subdata(in: start..<(start + length))
            let manifest: Data
            if method == 0 { manifest = encoded }
            else { manifest = try (encoded as NSData).decompressed(using: .zlib) as Data }
            guard manifest.count == decoded else { throw HostFailure(message: "Incomplete package manifest") }
            let value = JSON(try JSONSerialization.jsonObject(with: manifest))
            let rulers = value["objects"].array.filter { $0["type"].string == "capy.guides/1" }.flatMap { $0["data"]["rulers"].array }
            return JSON(rulers.map { ruler in
                var geometry = ruler["geometry"].object
                for key in ["start", "end", "center"] where !ruler["geometry"][key].isNull {
                    let point = ruler["geometry"][key]
                    geometry[key] = ["x": point[0].number, "y": point[1].number]
                }
                return ["id": ruler["id"].raw, "geometry": geometry]
            })
        }
        offset = start + length
    }
    throw HostFailure(message: "Missing package manifest")
}
