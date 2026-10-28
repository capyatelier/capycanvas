#include "../CanvasModelUpdate.h"
#include <cassert>

int main(){
    winrt::init_apartment();
    using J=winrt::Windows::Data::Json::JsonObject;
    CanvasModelUpdate models;
    auto initial=J::Parse(LR"({"state":{"revision":1,"histogram":{"status":"old"},"workspace":{"layout":{"extent":240}}},"catalog":{"01":"literal","a/b":"key"},"rows":[{"value":1},{"value":2}]})");
    assert(models.Read(initial)==CanvasAllSections);
    initial.Insert(L"localization",J::Parse(LR"({"generation":1})"));
    models.Read(initial);
    assert(!models.Model().HasKey(L"localization"));
    auto old=models.Model();
    auto fields=J::Parse(LR"({"model_update":[[["state","revision"],2],[["state","histogram","status"],"ready"]]})");
    assert(models.Read(fields)==CanvasScopes);
    auto next=models.Model();
    assert(old.GetNamedObject(L"state").GetNamedObject(L"histogram").GetNamedString(L"status")==L"old");
    assert(next.GetNamedObject(L"state").GetNamedObject(L"histogram").GetNamedString(L"status")==L"ready");
    old.GetNamedObject(L"state").GetNamedObject(L"workspace").GetNamedObject(L"layout").Insert(L"extent",winrt::Windows::Data::Json::JsonValue::CreateNumberValue(999));
    assert(models.Model().GetNamedObject(L"state").GetNamedObject(L"workspace").GetNamedObject(L"layout").GetNamedNumber(L"extent")==240);
    models.Read(J::Parse(LR"({"model_update":[[["catalog","01"],"changed"],[["catalog","a/b"],"literal path"],[["rows","0","value"],3],[["rows","1","value"],4]]})"));
    next=models.Model();
    assert(next.GetNamedObject(L"catalog").GetNamedString(L"01")==L"changed");
    assert(next.GetNamedObject(L"catalog").GetNamedString(L"a/b")==L"literal path");
    assert(next.GetNamedArray(L"rows").GetObjectAt(0).GetNamedNumber(L"value")==3);
    assert(next.GetNamedArray(L"rows").GetObjectAt(1).GetNamedNumber(L"value")==4);
    assert(old.GetNamedArray(L"rows").GetObjectAt(0).GetNamedNumber(L"value")==1);
    models.Read(J::Parse(LR"({"model_update":[[["rows"],[7]],[["state","histogram"],null]]})"));
    next=models.Model();
    assert(next.GetNamedArray(L"rows").Size()==1&&next.GetNamedArray(L"rows").GetNumberAt(0)==7);
    assert(next.GetNamedObject(L"state").GetNamedValue(L"histogram").ValueType()==winrt::Windows::Data::Json::JsonValueType::Null);
    return 0;
}
