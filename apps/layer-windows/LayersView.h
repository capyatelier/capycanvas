#pragma once
#include "UiControls.h"
#include "LayerThumbnails.h"
#include "NativeMenus.h"
#include <winrt/Windows.ApplicationModel.DataTransfer.h>
#include <optional>

winrt::Microsoft::UI::Xaml::FrameworkElement LayersPanel(
    std::shared_ptr<CapyUi::WorkspaceData> const&,CapyUi::Bindings&);
namespace CapyLayers {
using namespace CapyUi;
inline J layerAction(J const& operation){return O({{L"type",S(L"layer")},{L"action",operation}});}
inline hstring epochOf(std::shared_ptr<WorkspaceData> const& data){
    return to_hstring(uint64_t(num(object(data->state,L"document_file"),L"epoch")));
}
struct LayersView;
struct LayerRow : std::enable_shared_from_this<LayerRow> {
    std::weak_ptr<LayersView> owner;
    std::shared_ptr<WorkspaceData> data;
    double id=0;
    hstring epoch;
    Border root;
    Grid body;
    Button eye{nullptr},check{nullptr},content{nullptr},mask{nullptr},link{nullptr},name{nullptr},grip{nullptr};
    Border indent,clip,dropMark;
    Grid contentTile,maskTile;
    Image contentImage,maskImage,lockImage;
    Canvas contentCorners,maskCorners;
    TextBlock title{nullptr},meta{nullptr};
    TextBox rename;
    bool renaming=false,committing=false;
    hstring imageKey,iconKey;
    J model()const;
    bool current()const;
    void action(J operation);
    void init();
    void refresh();
    void thumbnails(std::vector<LayerThumbnail>& visible);
    void commit(bool cancel);
    void context(bool mask,UIElement const& anchor);
    void highlight(int position);
    void dragSource(UIElement const& source);
};
struct ElementFactory : implements<ElementFactory,IElementFactory> {
    std::weak_ptr<LayersView> owner;
    UIElement GetElement(ElementFactoryGetArgs const&);
    void RecycleElement(ElementFactoryRecycleArgs const&);
};
struct LayersView : std::enable_shared_from_this<LayersView> {
    std::shared_ptr<WorkspaceData> data;
    Grid root,values;
    StackPanel header,tools,footer;
    ScrollView list;
    ItemsRepeater repeater;
    ComboBox blend;
    ContentControl opacityGate;
    Bindings opacityBindings;
    hstring opacityKey,epoch;
    double opacityLayer=-1;
    std::vector<std::function<void(J,J)>> controls;
    Windows::Foundation::Collections::IObservableVector<Windows::Foundation::IInspectable> source{
        single_threaded_observable_vector<Windows::Foundation::IInspectable>()};
    std::map<void*,std::shared_ptr<LayerRow>> rows;
    Microsoft::UI::Dispatching::DispatcherQueueTimer timer{nullptr},dragTimer{nullptr};
    double dragSpeed=0;
    MenuFlyout menu{nullptr};
    uint64_t menuGeneration=0;
    std::optional<double> dragged;
    hstring dragEpoch;
    ~LayersView();
    J view()const{return object(data->state,L"layer_tools");}
    J editing()const{return object(view(),L"editing_layer");}
    void action(J operation){if(!data->updating)data->dispatchDocument(layerAction(operation),epochOf(data));}
    void init();
    void refresh();
    void preview();
    void context(double id,bool mask,UIElement const& anchor);
    void clearDrag();
    bool dragCurrent()const;
};
}
