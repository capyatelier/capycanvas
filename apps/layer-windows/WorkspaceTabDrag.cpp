#include "pch.h"
#include "WorkspaceTabDrag.h"
#include "WorkspaceGeometry.h"
#include <winrt/Microsoft.UI.Composition.h>
#include <winrt/Windows.UI.ViewManagement.h>

using namespace CapyUi;
using namespace Microsoft::UI::Composition;
namespace {
FrameworkElement shellOf(FrameworkElement const& tab){
    auto parent=VisualTreeHelper::GetParent(tab).try_as<FrameworkElement>();
    if(parent){
        auto value=parent.Tag().try_as<Windows::Foundation::IPropertyValue>();
        if(value&&value.Type()==Windows::Foundation::PropertyType::String){
            auto name=value.GetString();
            if(name==L"active-panel-tab-shell"||name==L"panel-tab-shell")return parent;
        }
    }
    return tab;
}
}
struct WorkspaceTabDrag::Impl {
    struct Entry {
        weak_ref<FrameworkElement> source;
        J hit,model;
        hstring panel;
        double opacity=1,target=0;
        bool active=false;
        FrameworkElement copy{nullptr};
        Vector3KeyFrameAnimation animation{nullptr};
    };
    std::shared_ptr<WorkspaceData> data;
    Canvas root{nullptr},overlay;
    std::vector<Entry> entries;
    J clip;
    uint32_t group=0,source=0;
    bool visible=false,animate=true;
    Impl(std::shared_ptr<WorkspaceData> state,Canvas workspace):data(std::move(state)),root(workspace){
        overlay.IsHitTestVisible(false);Canvas::SetZIndex(overlay,9000);
        AutomationProperties::SetAutomationId(overlay,L"workspace-tab-preview");
        AutomationProperties::SetName(overlay,L"Tab drag preview");
        AutomationProperties::SetAccessibilityView(overlay,Microsoft::UI::Xaml::Automation::Peers::AccessibilityView::Raw);
    }
    ~Impl(){clear();}
    void clear(){
        for(auto& entry:entries){
            if(entry.animation)entry.copy.StopAnimation(entry.animation);
            if(visible)if(auto original=entry.source.get())original.Opacity(entry.opacity);
        }
        uint32_t index;if(root.Children().IndexOf(overlay,index))root.Children().RemoveAt(index);
        overlay.Children().Clear();entries.clear();clip=J{};visible=false;
    }
    void grab(J const& tab,std::vector<weak_ref<FrameworkElement>> const& candidates){
        clear();if(!tab.Size())return;
        group=uint32_t(num(tab,L"group"));source=uint32_t(num(tab,L"index"));
        std::map<uint32_t,Entry> captured;
        for(auto const& weak:candidates){
            auto element=weak.get();if(!element||!element.IsLoaded()||element.ActualWidth()<=0)continue;
            auto tag=element.Tag().try_as<J>();if(!tag)continue;auto current=object(tag,L"workspace_tab");
            if(!current.Size()||num(current,L"group")!=group)continue;
            auto index=uint32_t(num(current,L"index"));
            auto bounds=element.TransformToVisual(root).TransformBounds({0,0,float(element.ActualWidth()),float(element.ActualHeight())});
            Entry entry;auto shell=shellOf(element);entry.source=make_weak(shell);entry.opacity=shell.Opacity();
            entry.panel=str(current,L"panel");entry.model=find(array(data->model,L"panels"),L"id",entry.panel);
            if(!entry.model.Size())continue;
            entry.hit=O({{L"group",N(group)},{L"index",N(index)},{L"bounds",rectangle(bounds)}});
            if(auto value=shell.Tag().try_as<Windows::Foundation::IPropertyValue>();value&&value.Type()==Windows::Foundation::PropertyType::String)
                entry.active=value.GetString()==L"active-panel-tab-shell";
            captured.insert_or_assign(index,std::move(entry));
            if(index==source){
                auto parent=VisualTreeHelper::GetParent(element);
                while(parent&&parent!=root){
                    if(auto strip=parent.try_as<ScrollViewer>()){clip=rectangle(visibleBounds(strip,root));break;}
                    parent=VisualTreeHelper::GetParent(parent);
                }
            }
        }
        if(!captured.contains(source)||num(clip,L"width")<=0||num(clip,L"height")<=0){clear();return;}
        for(auto& [index,entry]:captured){
            if(index!=entries.size()){clear();return;}
            entries.push_back(std::move(entry));
        }
    }
    J begin(){
        if(entries.empty())return {};
        place(overlay,clip);overlay.Background(data->brush(L"tabbar"));
        RectangleGeometry mask;mask.Rect({0,0,float(num(clip,L"width")),float(num(clip,L"height"))});overlay.Clip(mask);
        animate=Windows::UI::ViewManagement::UISettings().AnimationsEnabled();
        A hits;
        for(uint32_t i=0;i<entries.size();++i){
            auto& entry=entries[i];hits.Append(entry.hit);
            StackPanel row;row.Orientation(Orientation::Horizontal);row.Spacing(6);
            row.HorizontalAlignment(HorizontalAlignment::Center);row.VerticalAlignment(VerticalAlignment::Center);
            auto presentation=object(entry.model,L"tab");
            if(flag(presentation,L"show_icon"))row.Children().Append(icon(str(entry.model,L"icon"),data->theme()));
            if(flag(presentation,L"show_name"))row.Children().Append(label(data,str(entry.model,L"title"),true));
            Border content;content.Padding({8,4,8,4});content.Child(row);
            auto copy=panelTabShell(data,content,entry.active);
            auto bounds=rectangle(object(entry.hit,L"bounds"));bounds.X-=float(num(clip,L"x"));bounds.Y-=float(num(clip,L"y"));
            place(copy,rectangle(bounds));entry.copy=copy;
            Canvas::SetZIndex(copy,i==source?2:entry.active?1:0);overlay.Children().Append(copy);
            if(auto original=entry.source.get())original.Opacity(0);
        }
        visible=true;root.Children().Append(overlay);
        return O({{L"tabs",hits},{L"clip",clip}});
    }
    void update(J const& preview){
        if(!visible)return;
        if(!preview.Size()){clear();return;}
        auto offsets=array(preview,L"offsets");
        if(offsets.Size()!=entries.size()){clear();return;}
        for(uint32_t i=0;i<entries.size();++i){
            auto& entry=entries[i];
            double target=i==source?num(object(preview,L"bounds"),L"x")-num(object(entry.hit,L"bounds"),L"x"):
                num(offsets.GetObjectAt(i),L"x");
            if(target==entry.target)continue;entry.target=target;
            if(i!=source&&animate){
                // Composition animates presentation only; no UI timer, layout
                // pass or canvas submission is needed for the in-between frames.
                auto compositor=CompositionTarget::GetCompositorForCurrentThread();
                auto move=compositor.CreateVector3KeyFrameAnimation();move.Target(L"Translation");
                move.InsertExpressionKeyFrame(0,L"this.StartingValue");
                auto easing=compositor.CreateCubicBezierEasingFunction({.215f,.61f},{.355f,1.f});
                move.InsertKeyFrame(1,{float(target),0,0},easing);move.Duration(std::chrono::milliseconds(120));
                entry.animation=move;entry.copy.StartAnimation(move);
            }else{
                if(entry.animation){entry.copy.StopAnimation(entry.animation);entry.animation=nullptr;}
                entry.copy.Translation({float(target),0,0});
            }
        }
        AutomationProperties::SetItemStatus(overlay,preview.Stringify());
    }
    void refresh(std::vector<weak_ref<FrameworkElement>> const& candidates){
        if(!visible)return;
        // Theme/configuration projection can replace a widget during capture.
        // Hide its replacement while keeping the original grab geometry.
        for(auto const& weak:candidates){
            auto element=weak.get();if(!element||!element.IsLoaded())continue;
            auto tag=element.Tag().try_as<J>();if(!tag)continue;auto tab=object(tag,L"workspace_tab");
            if(!tab.Size()||num(tab,L"group")!=group)continue;
            auto index=uint32_t(num(tab,L"index"));if(index>=entries.size())continue;
            auto& entry=entries[index];if(str(tab,L"panel")!=entry.panel)continue;
            auto shell=shellOf(element);if(shell==entry.source.get())continue;
            if(auto previous=entry.source.get())previous.Opacity(entry.opacity);
            entry.source=make_weak(shell);entry.opacity=shell.Opacity();shell.Opacity(0);
        }
        uint32_t index;if(!root.Children().IndexOf(overlay,index))root.Children().Append(overlay);
    }
};
WorkspaceTabDrag::WorkspaceTabDrag(std::shared_ptr<WorkspaceData> data,Canvas root):impl(std::make_unique<Impl>(std::move(data),root)){}
WorkspaceTabDrag::~WorkspaceTabDrag()=default;
void WorkspaceTabDrag::Grab(J const& tab,std::vector<weak_ref<FrameworkElement>> const& tabs){impl->grab(tab,tabs);}
J WorkspaceTabDrag::Begin(){return impl->begin();}
void WorkspaceTabDrag::Update(J const& preview){impl->update(preview);}
void WorkspaceTabDrag::Refresh(std::vector<weak_ref<FrameworkElement>> const& tabs){impl->refresh(tabs);}
void WorkspaceTabDrag::Clear(){impl->clear();}
