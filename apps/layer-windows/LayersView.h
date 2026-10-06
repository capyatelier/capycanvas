#pragma once
#include "UiControls.h"
#include "LayerThumbnails.h"
#include "NativeMenus.h"
#include "LayerRowDrag.h"
#include <winrt/Microsoft.UI.Xaml.Shapes.h>
#include <winrt/Microsoft.UI.Xaml.Media.Animation.h>
#include <optional>

winrt::Microsoft::UI::Xaml::FrameworkElement LayersPanel(
    std::shared_ptr<CapyUi::WorkspaceData> const&,CapyUi::Bindings&,std::function<double()>* contentHeight=nullptr,std::function<CapyUi::J()>* scrollMetrics=nullptr);
namespace CapyLayers {
using namespace CapyUi;
inline J layerAction(J const& operation){return O({{L"type",S(L"layer")},{L"action",operation}});}
inline hstring epochOf(std::shared_ptr<WorkspaceData> const& data){
    return to_hstring(uint64_t(num(object(data->state,L"document_file"),L"epoch")));
}
struct LayersView;
struct ThumbnailEdge {
    Shapes::Path base,outline;
    ScaleTransform scale;
    Animation::Storyboard animation{nullptr};
    std::optional<bool> selected;
    void init(Canvas const& frame);
    void update(std::shared_ptr<WorkspaceData> const& data,bool editing);
    void stop();
};
struct LayerRow : std::enable_shared_from_this<LayerRow> {
    std::weak_ptr<LayersView> owner;
    std::shared_ptr<WorkspaceData> data;
    double id=0;
    hstring epoch;
    Border root;
    Grid body,swipeFrame;
    Button swipeDelete{nullptr};
    TranslateTransform swipeTransform;
    double swipeOffset=0;
    void swipe(double offset);
    Button eye{nullptr},check{nullptr},content{nullptr},load{nullptr},mask{nullptr},link{nullptr},name{nullptr},grip{nullptr};
    Border indent,dropMark,contentSymbol,groupMode;
    Grid contentTile,maskTile;
    Image lockImage;
    ContentControl contentThumbnail,maskThumbnail;
    ThumbnailEdge contentEdge,maskEdge;
    ImageBrush contentPreview,maskPreview;
    Canvas contentFrame,maskFrame;
    TextBlock title{nullptr},meta{nullptr};
    TextBox rename;
    bool renaming=false,committing=false;
    winrt::event_token renameFocus{};
    void focusRename();
    hstring imageKey,iconKey,captionTitle;
    J model()const;
    bool current()const;
    bool clickAllowed()const;
    bool contextAllowed()const;
    void action(J operation);
    void init();
    void refresh();
    void thumbnails(std::vector<LayerThumbnail>& visible);
    void commit(bool cancel);
    void context(bool mask,UIElement const& anchor);
    bool loadThumbnail(bool mask);
    void editFill();
    void highlight(int position, bool attachment = false);
};
struct ElementFactory : implements<ElementFactory,IElementFactory> {
    std::weak_ptr<LayersView> owner;
    UIElement GetElement(ElementFactoryGetArgs const&);
    void RecycleElement(ElementFactoryRecycleArgs const&);
};
struct LayersView : std::enable_shared_from_this<LayersView> {
    std::shared_ptr<WorkspaceData> data;
    Grid root,values,footerFrame,listFrame;
    Canvas connectionOverlay;
    hstring connectionKey,connectionTheme;
    std::vector<Shapes::Line> connectionLines;
    std::vector<Image> connectionGlyphs;
    CompositionTarget::Rendering_revoker connectionFrame;
    UIElement outsideSurface{nullptr};
    PointerEventHandler outsidePress{nullptr};
    StackPanel header,tools,footer;
    ScrollView list;
    ItemsRepeater repeater;
    Button blend{nullptr};
    TextBlock blendLabel{nullptr};
    ContentControl opacityGate;
    Bindings opacityBindings;
    hstring opacityKey,epoch;
    double opacityLayer=-1;
    std::vector<std::function<void(J,J)>> controls;
    Windows::Foundation::Collections::IObservableVector<Windows::Foundation::IInspectable> source{
        single_threaded_observable_vector<Windows::Foundation::IInspectable>()};
    std::map<void*,std::shared_ptr<LayerRow>> rows;
    Microsoft::UI::Dispatching::DispatcherQueueTimer timer{nullptr};
    std::unique_ptr<LayerRowDrag> pickup;
    MenuFlyout menu{nullptr};
    uint64_t menuGeneration=0;
    bool menuOpen=false,menuPending=false;
    std::optional<double> menuTarget;
    ~LayersView();
    J view()const{return object(data->state,L"layer_tools");}
    J editing()const{return object(view(),L"editing_layer");}
    void action(J operation){if(!data->updating)data->dispatchDocument(layerAction(operation),epochOf(data));}
    void init();
    void refresh();
    void preview();
    void connections();
    void showMenu(J spec, FrameworkElement const& anchor);
    void layoutConnections();
    void context(double id,bool mask,UIElement const& anchor,
        std::optional<Windows::Foundation::Point> at={},bool holding=false,bool blendMenu=false);
};
}
