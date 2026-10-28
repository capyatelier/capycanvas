#pragma once
#include "UiControls.h"
#include <chrono>
#include <set>
#include <tuple>

struct ScrollIndicatorsFixture:std::enable_shared_from_this<ScrollIndicatorsFixture>{
    using J=CapyUi::J;
    winrt::Microsoft::UI::Xaml::Window window{nullptr};
    winrt::Microsoft::UI::Xaml::Controls::Grid root;
    winrt::Microsoft::UI::Xaml::Controls::ScrollView view;
    winrt::Microsoft::UI::Xaml::Controls::ScrollViewer viewer;
    winrt::Microsoft::UI::Xaml::Controls::StackPanel viewBody,viewerBody;
    winrt::Microsoft::UI::Xaml::Controls::TextBlock evidence;
    winrt::Microsoft::UI::Dispatching::DispatcherQueueTimer timer{nullptr};
    std::set<void*> watched;
    CapyUi::A writes,presses;
    winrt::Microsoft::UI::Xaml::Controls::Grid hosts[2];
    uint32_t churnCycle=0,churnPhase=0,reparents=0,replacements=0,stateTransitions=0;bool churning=false;winrt::hstring churnError;uint32_t undefinedRequests=0;bool undefinedView=false,undefinedViewer=false;
    static winrt::Microsoft::UI::Xaml::FrameworkElement templateRoot(winrt::Microsoft::UI::Xaml::Controls::Control const& control){
        using namespace winrt::Microsoft::UI::Xaml;
        return Media::VisualTreeHelper::GetChildrenCount(control)?Media::VisualTreeHelper::GetChild(control,0).try_as<FrameworkElement>():nullptr;
    }
    static bool visible(winrt::Microsoft::UI::Xaml::FrameworkElement const& element){
        using namespace winrt::Microsoft::UI::Xaml;
        if(!element||element.ActualWidth()<=0||element.ActualHeight()<=0)return false;
        for(auto node=element.as<DependencyObject>();node;node=Media::VisualTreeHelper::GetParent(node))
            if(auto item=node.try_as<UIElement>();item&&item.Visibility()!=Visibility::Visible)return false;
        return true;
    }
    J inspect(winrt::Microsoft::UI::Xaml::Controls::Control const& control,wchar_t const* family){
        using namespace CapyUi;using namespace winrt::Microsoft::UI::Xaml;
        auto frame=templateRoot(control);A groups;
        auto controlBounds=control.TransformToVisual(root).TransformBounds({0,0,float(control.ActualWidth()),float(control.ActualHeight())});
        if(frame)for(auto group:VisualStateManager::GetVisualStateGroups(frame))groups.Append(S(group.Name()));
        J result=O({{L"root",N(double(reinterpret_cast<uintptr_t>(winrt::get_abi(frame))))},{L"groups",groups}});
        result.Insert(L"bounds",O({{L"x",N(controlBounds.X)},{L"y",N(controlBounds.Y)},{L"width",N(controlBounds.Width)},{L"height",N(controlBounds.Height)}}));
        for(auto direction:{L"Horizontal",L"Vertical"}){
            auto name=std::wstring(family==std::wstring_view(L"view")?L"PART_":L"")+direction+L"ScrollBar";
            auto bar=frame?frame.FindName(name).try_as<Controls::Primitives::ScrollBar>():nullptr;
            J state=O({{L"id",N(double(reinterpret_cast<uintptr_t>(winrt::get_abi(bar))))},{L"visible",B(visible(bar))},{L"mode",N(bar?int(bar.IndicatorMode()):-1)}});
            if(bar){
                auto bounds=bar.TransformToVisual(root).TransformBounds({0,0,float(bar.ActualWidth()),float(bar.ActualHeight())});
                state.Insert(L"bounds",O({{L"x",N(bounds.X)},{L"y",N(bounds.Y)},{L"width",N(bounds.Width)},{L"height",N(bounds.Height)}}));
                state.Insert(L"minimum",N(bar.Minimum()));state.Insert(L"maximum",N(bar.Maximum()));state.Insert(L"value",N(bar.Value()));state.Insert(L"viewport",N(bar.ViewportSize()));
                if(auto surface=templateRoot(bar))if(auto thumb=surface.FindName(L"VerticalThumb").try_as<FrameworkElement>();thumb&&visible(thumb)){
                    auto b=thumb.TransformToVisual(root).TransformBounds({0,0,float(thumb.ActualWidth()),float(thumb.ActualHeight())});
                    state.Insert(L"thumb",O({{L"x",N(b.X)},{L"y",N(b.Y)},{L"width",N(b.Width)},{L"height",N(b.Height)}}));
                }
                if(watched.insert(winrt::get_abi(bar)).second){
                    bar.RegisterPropertyChangedCallback(Controls::Primitives::ScrollBar::IndicatorModeProperty(),[weak=weak_from_this(),family=winrt::hstring(family),direction=winrt::hstring(direction)](auto&& sender,auto&&){
                        if(auto self=weak.lock()){
                            auto value=sender.template as<Controls::Primitives::ScrollBar>();
                            if(self->writes.Size()==64)self->writes.RemoveAt(0);
                            self->writes.Append(O({{L"family",S(family)},{L"direction",S(direction)},{L"id",N(double(reinterpret_cast<uintptr_t>(winrt::get_abi(value))))},{L"mode",N(int(value.IndicatorMode()))}}));
                        }
                    });
                }
            }
            result.Insert(direction==std::wstring_view(L"Vertical")?L"vertical":L"horizontal",state);
        }
        return result;
    }
    void status(){
        using namespace CapyUi;using namespace winrt::Microsoft::UI::Xaml;
        auto a=inspect(view,L"view"),b=inspect(viewer,L"viewer");
        auto anchor=evidence.TransformToVisual(root).TransformPoint({0,0});
        for(auto const& [state,offset,extent,viewport]:{std::tuple{a,view.VerticalOffset(),view.ExtentHeight(),view.ViewportHeight()},std::tuple{b,viewer.VerticalOffset(),viewer.ExtentHeight(),viewer.ViewportHeight()}}){
            state.Insert(L"offset",N(offset));state.Insert(L"extent",N(extent));state.Insert(L"viewport",N(viewport));
        }
        Automation::AutomationProperties::SetItemStatus(evidence,O({{L"raster",N(root.XamlRoot().RasterizationScale())},{L"anchor",O({{L"x",N(anchor.X)},{L"y",N(anchor.Y)}})},{L"mouse",B(MouseScrollbars::current().mouse)},{L"view",a},{L"viewer",b},{L"writes",writes},{L"presses",presses},{L"undefined",O({{L"requests",N(undefinedRequests)},{L"view",B(undefinedView)},{L"viewer",B(undefinedViewer)}})},{L"churn",O({{L"error",S(churnError)},{L"running",B(churning)},{L"cycle",N(churnCycle)},{L"reparents",N(reparents)},{L"replacements",N(replacements)},{L"transitions",N(stateTransitions)}})}}).Stringify());
    }
    void content(int count,double height){
        using namespace winrt;using namespace Microsoft::UI::Xaml;
        for(auto const& body:{viewBody,viewerBody}){
            body.Children().Clear();for(int i=0;i<count;++i){Controls::TextBlock row;row.Text(L"Row "+to_hstring(i));row.Height(40);row.FontSize(18);body.Children().Append(row);}
        }
        view.Height(height);viewer.Height(height);root.UpdateLayout();status();
    }
    void configure(){
        using namespace CapyUi;using namespace winrt;using namespace Microsoft::UI::Xaml;
        view.Width(260);viewer.Width(260);view.Height(260);viewer.Height(260);
        view.HorizontalScrollBarVisibility(Controls::ScrollingScrollBarVisibility::Hidden);view.VerticalScrollBarVisibility(Controls::ScrollingScrollBarVisibility::Auto);
        viewer.HorizontalScrollBarVisibility(Controls::ScrollBarVisibility::Disabled);viewer.VerticalScrollBarVisibility(Controls::ScrollBarVisibility::Auto);
        viewer.VerticalScrollMode(Controls::ScrollMode::Enabled);viewer.IsTabStop(true);view.IsTabStop(true);
        mouseScrollbars(view);mouseScrollbars(viewer);view.Content(viewBody);viewer.Content(viewerBody);
        Automation::AutomationProperties::SetAutomationId(view,L"indicator-view");Automation::AutomationProperties::SetAutomationId(viewer,L"indicator-viewer");

    }
    void churn(){
        using namespace winrt::Microsoft::UI::Xaml;
        if(!churning)return;
        if(churnPhase==0){
            view.ScrollTo(0,churnCycle%2?0:300,nullptr);
            viewer.ChangeView(nullptr,double(churnCycle%2?0:300),nullptr,false);
            for(auto const& control:{view.as<Controls::Control>(),viewer.as<Controls::Control>()})if(auto frame=templateRoot(control)){
                for(auto name:{L"PART_VerticalScrollBar",L"VerticalScrollBar"})if(auto bar=frame.FindName(name).try_as<Controls::Primitives::ScrollBar>()){
                    bar.IndicatorMode(Controls::Primitives::ScrollingIndicatorMode::TouchIndicator);
                    bar.IndicatorMode(Controls::Primitives::ScrollingIndicatorMode::MouseIndicator);++stateTransitions;
                }
            }
            churnPhase=1;return;
        }
        hosts[0].Children().Clear();hosts[1].Children().Clear();
        if(churnCycle%3==2){view.Content(nullptr);viewer.Content(nullptr);view=Controls::ScrollView();viewer=Controls::ScrollViewer();configure();++replacements;}
        else ++reparents;
        hosts[churnCycle%2].Children().Append(view);hosts[1-churnCycle%2].Children().Append(viewer);
        churnPhase=0;if(++churnCycle==48)churning=false;
    }
    void open(){
        SetErrorMode(SEM_FAILCRITICALERRORS|SEM_NOGPFAULTERRORBOX);
        using namespace CapyUi;using namespace winrt;using namespace Microsoft::UI::Xaml;
        wchar_t theme[16]{};GetEnvironmentVariableW(L"CAPY_FEEDBACK_THEME",theme,16);
        root.RequestedTheme(std::wstring_view(theme)==L"dark"?ElementTheme::Dark:ElementTheme::Light);
        root.Background(SolidColorBrush(std::wstring_view(theme)==L"dark"?Windows::UI::Color{255,35,35,35}:Windows::UI::Color{255,237,237,237}));
        Controls::StackPanel page;page.Margin({24,24,24,24});page.Spacing(16);page.HorizontalAlignment(HorizontalAlignment::Left);page.VerticalAlignment(VerticalAlignment::Top);
        Controls::StackPanel row;row.Orientation(Controls::Orientation::Horizontal);row.Spacing(40);
        configure();
        hosts[0].Children().Append(view);hosts[1].Children().Append(viewer);row.Children().Append(hosts[0]);row.Children().Append(hosts[1]);page.Children().Append(row);
        Controls::StackPanel actions;actions.Orientation(Controls::Orientation::Horizontal);actions.Spacing(8);
        for(auto const& [id,label,action]:{std::tuple{L"indicator-long",L"Long",0},std::tuple{L"indicator-short",L"Short",1},std::tuple{L"indicator-fit",L"Fit",2},std::tuple{L"indicator-focus-view",L"Focus view",4},std::tuple{L"indicator-focus-viewer",L"Focus viewer",5},std::tuple{L"indicator-churn",L"Churn",6},std::tuple{L"indicator-undefined",L"Unknown",7}}){
            Controls::Button button;button.Content(box_value(label));Automation::AutomationProperties::SetAutomationId(button,id);
            button.Click([weak=weak_from_this(),action](auto&&,auto&&){if(auto self=weak.lock()){if(action==7){self->undefinedView=VisualStateManager::GoToState(self->view,L"CapyUndefinedIndicatorState",false);self->undefinedViewer=VisualStateManager::GoToState(self->viewer,L"CapyUndefinedIndicatorState",false);++self->undefinedRequests;self->status();}else if(action==6){self->churnCycle=0;self->churnPhase=0;self->reparents=0;self->replacements=0;self->stateTransitions=0;self->churnError=L"";self->churning=true;}else if(action==4)self->view.Focus(FocusState::Keyboard);else if(action==5)self->viewer.Focus(FocusState::Keyboard);else self->content(action==0?20:action==1?10:2,action==1?170:260);}});actions.Children().Append(button);
        }
        page.Children().Append(actions);evidence.Text(L"Scroll indicator fixture");Automation::AutomationProperties::SetAutomationId(evidence,L"indicator-status");page.Children().Append(evidence);root.Children().Append(page);
        root.Loaded([weak=weak_from_this()](auto&&,auto&&){if(auto self=weak.lock()){auto scale=float(self->root.XamlRoot().RasterizationScale());self->window.AppWindow().ResizeClient({int32_t(700*scale),int32_t(450*scale)});self->content(20,260);}});
        root.AddHandler(UIElement::PointerPressedEvent(),box_value(Input::PointerEventHandler([weak=weak_from_this()](auto&&,Input::PointerRoutedEventArgs const& e){if(auto self=weak.lock()){
            auto source=e.OriginalSource().try_as<FrameworkElement>();auto point=e.GetCurrentPoint(self->root).Position();hstring family;
            for(auto node=e.OriginalSource().try_as<DependencyObject>();node;node=Media::VisualTreeHelper::GetParent(node))if(auto element=node.try_as<UIElement>()){
                auto id=Automation::AutomationProperties::GetAutomationId(element);if(id==L"indicator-view"||id==L"indicator-viewer"){family=id;break;}
            }
            if(self->presses.Size()==16)self->presses.RemoveAt(0);
            self->presses.Append(O({{L"family",S(family)},{L"source",S(source?source.Name():L"")},{L"x",N(point.X)},{L"y",N(point.Y)}}));
        }})),true);
        window=Window();window.Content(root);window.AppWindow().Move({40,40});window.AppWindow().Resize({1100,850});window.Activate();
        timer=root.DispatcherQueue().CreateTimer();timer.Interval(std::chrono::milliseconds(30));timer.Tick([weak=weak_from_this()](auto&&,auto&&){if(auto self=weak.lock()){try{self->churn();}catch(winrt::hresult_error const& error){self->churnError=error.message();self->churning=false;}self->status();}});timer.Start();
        window.Closed([weak=weak_from_this()](auto&&,auto&&){if(auto self=weak.lock())self->timer.Stop();});
    }
};
