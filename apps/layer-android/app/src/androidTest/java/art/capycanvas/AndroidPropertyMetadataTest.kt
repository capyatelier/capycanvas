package art.capycanvas

import org.json.JSONArray
import org.json.JSONObject
import org.junit.Assert.*
import org.junit.Test

class AndroidPropertyMetadataTest {
    @Test fun equalSectionCaptionsKeepDistinctWireIdentityAndChoiceIndices() {
        val controls = JSONArray("""[
            {"key":"domain","section":"同じ","section_id":{"message":"common-cancel"},"kind":{"kind":"choice","options":["同じ","同じ"]},"value":{"kind":"choice","value":1}},
            {"key":"literal","section":"同じ","section_id":"同じ"},
            {"key":"object_text","section":"同じ","section_id":"{\"message\":\"common-cancel\"}"},
            {"key":"other","section":"同じ","section_id":{"message":"common-error"}}
        ]""").objects()
        assertEquals(4, controls.map(::propertySectionId).distinct().size)
        assertEquals(propertySectionId(controls[0]), propertySectionId(JSONObject(controls[0].toString())))
        val changedCaption = JSONObject(controls[0].toString()).put("section", "別の表示")
        assertEquals(propertySectionId(controls[0]), propertySectionId(changedCaption))
        assertEquals(propertySectionId(JSONObject()), propertySectionId(JSONObject().put("section_id", JSONObject.NULL)))
        assertEquals("domain", controls[0].getString("key"))
        assertEquals(1, controls[0].getJSONObject("value").getInt("value"))
        assertEquals(listOf("同じ", "同じ"), controls[0].getJSONObject("kind").array("options").values())
    }
}
