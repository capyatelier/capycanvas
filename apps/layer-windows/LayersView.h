#pragma once
#include "UiControls.h"
#include "LayerThumbnails.h"
#include "NativeMenus.h"
#include "LayerRowDrag.h"
#include "TextureDialog.h"
#include <winrt/Microsoft.UI.Xaml.Shapes.h>
#include <winrt/Microsoft.UI.Xaml.Media.Animation.h>
#include <optional>

namespace CapyLayers { struct LayersView; }

winrt::Microsoft::UI::Xaml::FrameworkElement LayersPanel(
    std::shared_ptr<CapyUi::WorkspaceData> const&,CapyUi::Bindings&,std::function<double()>* contentHeight=nullptr,std::function<CapyUi::J()>* scrollMetrics=nullptr,
    std::shared_ptr<CapyLayers::LayersView>* layerView=nullptr);
namespace CapyLayers {
using namespace CapyUi;
inline constexpr double LayerRowHeight=40;
inline constexpr Thickness LayerRowPadding{6,2,6,2};
inline J layerAction(J const& operation){return O({{L"type",S(L"layer")},{L"action",operation}});}
inline hstring epochOf(std::shared_ptr<WorkspaceData> const& data){
    return to_hstring(uint64_t(num(object(data->state,L"document_file"),L"epoch")));
}
inline J panelRow(J const& state,double id){return findId(array(state,L"layers"),id);}
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
    Grid swipeActions;
    std::map<std::wstring,Button> swipeButtons;
    hstring swipeKey;
    TranslateTransform swipeTransform;
    double swipeOffset=0;
    CompositionTarget::Rendering_revoker swipeFrames;
    void swipe(double offset,bool animate=true);
    void positionSwipe(double offset);
    double revealWidth()const;
    bool canSwipe()const;
    Button eye{nullptr},check{nullptr},content{nullptr},load{nullptr},mask{nullptr},link{nullptr},texture{nullptr},name{nullptr},grip{nullptr};
    Border indent,dropMark,contentSymbol,groupMode;
    Grid contentTile,maskTile;
    Image lockImage;
    ContentControl contentThumbnail,maskThumbnail;
    ThumbnailEdge contentEdge,maskEdge,textureEdge;
    std::shared_ptr<PaperThumbnail> texturePreview;
    ImageBrush contentPreview,maskPreview;
    Canvas contentFrame,maskFrame,textureFrame;
    TextBlock title{nullptr},meta{nullptr};
    TextBox rename;
    bool renaming=false,committing=false;
    winrt::event_token renameFocus{};
    void focusRename();
    hstring iconKey,captionTitle;
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
    bool loadThumbnail(bool mask,winrt::Windows::System::VirtualKeyModifiers modifiers);
    void editFill();
    void chooseTexture();
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
    Button blend{nullptr},addPaper{nullptr};
    hstring paperKey;
    TextBlock blendLabel{nullptr};
    ContentControl opacityGate;
    Bindings opacityBindings;
    Flyout opacityPopup{nullptr};
    Bindings opacityPopupBindings;
    hstring opacityPopupKey;
    hstring opacityKey,epoch;
    double opacityLayer=-1;
    std::optional<double> revealTarget;
    std::vector<std::function<void(J,J)>> controls;
    std::vector<std::pair<hstring,Button>> footerButtons;
    Windows::Foundation::Collections::IObservableVector<Windows::Foundation::IInspectable> source{
        single_threaded_observable_vector<Windows::Foundation::IInspectable>()};
    std::map<void*,std::shared_ptr<LayerRow>> rows;
    Microsoft::UI::Dispatching::DispatcherQueueTimer timer{nullptr};
    std::unique_ptr<LayerRowDrag> pickup;
    MenuFlyout menu{nullptr};
    OwnedPopup menuOwner;
    std::optional<double> menuTarget;
    ~LayersView();
    J view()const{return object(data->state,L"layer_tools");}
    J editing()const{return object(view(),L"editing_layer");}
    void action(J operation){if(!data->updating)data->dispatchDocument(layerAction(operation),epochOf(data));}
    bool full=true;
    void init();
    void refresh();
    void preview();
    J scrollAnchor()const;
    bool restoreAnchor(J const& anchor);
    void revealEditing();
    void connections();
    void showMenu(J spec, FrameworkElement const& anchor);
    void layoutConnections();
    void fitFooter();
    JsonArray footerOverflow();
    void context(double id,bool mask,UIElement const& anchor,
        std::optional<Windows::Foundation::Point> at={},bool holding=false,bool blendMenu=false,JsonArray extra=nullptr);
};
}
