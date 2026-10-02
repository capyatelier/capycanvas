import Foundation

/// Exercise the immutable transport boundary with Foundation-decoded and native
/// Swift containers. Optional files extend the check to captured wire fixtures.
@main struct JSONLookupChecks {
    static func verify(_ value: JSON, against raw: Any) throws -> Int {
        let expected = try JSONSerialization.data(withJSONObject: raw, options: [.fragmentsAllowed, .sortedKeys])
        precondition(value.stableKey == String(decoding: expected, as: UTF8.self))
        var count = 1
        if let object = raw as? [String: Any] {
            precondition(value.object.count == object.count && value.array.isEmpty)
            for (key, child) in object { count += try verify(value[key], against: child) }
            precondition(value[0].isNull)
        } else if let array = raw as? [Any] {
            precondition(value.array.count == array.count && value.object.isEmpty)
            for (index, child) in array.enumerated() { count += try verify(value[index], against: child) }
            precondition(value[-1].isNull && value[array.count].isNull && value[Int.max].isNull)
        } else {
            precondition(value[0].isNull && value["child"].isNull)
        }
        precondition(value["missing lookup fixture key"].isNull)
        return count
    }

    static func main() throws {
        let raw: [String: Any] = [
            "state": ["revision": UInt64.max, "enabled": true, "disabled": false,
                      "zoom": 0.1, "negative": -12, "nil": NSNull()],
            "rows": [["id": "undo", "label": "Crème 🖊️", "enabled": true],
                     ["id": "redo", "label": "", "enabled": false]],
            "emptyObject": [String: Any](), "emptyArray": [Any](),
            "": "empty key", "🖊️": "Unicode key"
        ]
        let native = JSON(raw)
        let decoded = try JSON.decode(native.encoded())
        let foundation = JSON(raw as NSDictionary)
        var count = 0
        for value in [native, decoded, foundation] {
            count += try verify(value, against: raw)
            precondition(value["state"]["revision"].uint == UInt64.max)
            precondition(value["state"]["enabled"].bool && !value["state"]["disabled"].bool)
            precondition(value["state"]["zoom"].number == 0.1)
            precondition(value["rows"][1]["id"].string == "redo")
            precondition(value[""].string == "empty key" && value["🖊️"].string == "Unicode key")
            let edited = value.replacing("state", with: JSON(["revision": UInt64(42)]))
            precondition(edited["state"]["revision"].uint == 42)
            precondition(value["state"]["revision"].uint == UInt64.max)
            precondition(edited["rows"].stableKey == value["rows"].stableKey)
            let roundTrip = try JSON.decode(edited.encoded())
            precondition(roundTrip.stableKey == edited.stableKey)
        }
        let controls = JSON([
            ["key": "domain", "section": "同じ", "section_id": ["message": "common-cancel"],
             "kind": ["kind": "choice", "options": ["同じ", "同じ"]],
             "value": ["kind": "choice", "value": 1]],
            ["key": "literal", "section": "同じ", "section_id": "同じ"],
            ["key": "object_text", "section": "同じ", "section_id": "{\"message\":\"common-cancel\"}"],
            ["key": "other", "section": "同じ", "section_id": ["message": "common-error"]]
        ] as [[String: Any]])
        for value in [controls, try JSON.decode(controls.encoded()), JSON(controls.raw as! NSArray)] {
            precondition(Set(value.array.map { $0["section_id"].stableKey }).count == 4)
            let changedCaption = value[0].replacing("section", with: JSON("別の表示"))
            precondition(value[0]["section_id"].stableKey == changedCaption["section_id"].stableKey)
            precondition(value[0]["key"].string == "domain" && value[0]["value"]["value"].uint == 1)
            precondition(value[0]["kind"]["options"].array.map(\.string) == ["同じ", "同じ"])
        }
        for path in CommandLine.arguments.dropFirst() {
            let value = try JSON.decode(String(contentsOfFile: path, encoding: .utf8))
            count += try verify(value, against: value.raw)
        }
        print("JSON lookup and round-trip checks passed for \(count) values")
    }
}
