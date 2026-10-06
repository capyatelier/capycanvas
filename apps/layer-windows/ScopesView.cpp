#include "pch.h"
#include "ScopesView.h"
#include <winrt/Microsoft.UI.Xaml.Shapes.h>
#include <winrt/Microsoft.UI.Xaml.Media.Imaging.h>
#include <winrt/Windows.Storage.Streams.h>
#include <robuffer.h>
#include <array>
#include <cstring>
namespace CapyUi {
using Plot=std::vector<std::pair<uint32_t,std::vector<float>>>;
struct ScopeFeed : std::enable_shared_from_this<ScopeFeed> {
    std::weak_ptr<WorkspaceData> owner;
    double requested=-1;bool busy=false;uint64_t generation=0;
    std::array<winrt::Windows::UI::Color,4> colors{};
    Plot histogram,tonal;
    PreviewPacket waveform;int waveformWidth=0,waveformHeight=0;
    std::array<int,2> waveformSize{},requestedSize{};
    std::vector<std::function<bool()>> views;
    static Plot plot(A const& source){
        Plot result;
        for(auto entry:source){
            auto pair=entry.GetArray();if(pair.Size()!=2)continue;
            auto bins=pair.GetArrayAt(1);std::vector<float> values;values.reserve(bins.Size());
            for(auto bin:bins)values.push_back(float(bin.GetNumber()));
            result.emplace_back(uint32_t(pair.GetNumberAt(0)),std::move(values));
        }
        return result;
    }
    void watch(std::function<bool()> view){views.push_back(std::move(view));}
    void refresh(){
        auto data=owner.lock();if(!data||busy||!data->query)return;
        auto revision=num(data->model,L"windows_scopes",-1);
        if(revision<0||(revision==requested&&requestedSize==waveformSize))return;
        auto dispatcher=Microsoft::UI::Dispatching::DispatcherQueue::GetForCurrentThread();if(!dispatcher)return;
        auto request=O({{L"type",S(L"scopes")}});
        if(waveformSize[0]>0&&waveformSize[1]>0){A size;size.Append(N(waveformSize[0]));size.Append(N(waveformSize[1]));request.Insert(L"size",size);}
        busy=true;requested=revision;requestedSize=waveformSize;
        bool queued=data->query(CanvasQueryKind::Workspace,to_string(request.Stringify()),[dispatcher,weak=weak_from_this()](PreviewPacket packet){
            dispatcher.TryEnqueue([weak,packet=std::move(packet)]{if(auto self=weak.lock())self->receive(packet);});
        });
        if(!queued){busy=false;requested=-1;}
    }
    void receive(PreviewPacket const& packet){
        busy=false;
        if(packet)try{
            auto result=object(J::Parse(to_hstring(capy_preview_metadata(packet.get()))),L"result");
            if(result.Size()){
                auto palette=array(result,L"colors");
                for(uint32_t i=0;i<4&&i<palette.Size();++i){
                    auto rgb=palette.GetArrayAt(i);
                    colors[i]={255,uint8_t(rgb.GetNumberAt(0)),uint8_t(rgb.GetNumberAt(1)),uint8_t(rgb.GetNumberAt(2))};
                }
                histogram=plot(array(result,L"histogram"));tonal=plot(array(result,L"tonal_histogram"));
                auto extent=result.GetNamedValue(L"waveform",JsonValue::CreateNullValue());
                if(extent.ValueType()==JsonValueType::Array){
                    waveform=packet;waveformWidth=int(extent.GetArray().GetNumberAt(0));waveformHeight=int(extent.GetArray().GetNumberAt(1));
                }else{waveform.reset();waveformWidth=waveformHeight=0;}
                ++generation;
            }
        }catch(hresult_error const& error){OutputDebugStringW(error.message().c_str());}
        std::erase_if(views,[](auto const& view){return !view();});
        refresh();
    }
};
}
namespace {
using namespace CapyScopes;
std::shared_ptr<ScopeFeed> feed(std::shared_ptr<WorkspaceData> const& data){
    if(!data->scopes){data->scopes=std::make_shared<ScopeFeed>();data->scopes->owner=data;}
    return data->scopes;
}
void drawPlot(Canvas const& target,Plot const& plot,std::array<winrt::Windows::UI::Color,4> const& colors,double width,double height){
    target.Children().Clear();if(width<=0||height<=0)return;
    for(auto const& [channel,bins]:plot){
        if(channel>3||bins.empty())continue;
        std::vector<winrt::Windows::Foundation::Point> points;points.reserve(bins.size()*2+2);
        double step=width/double(bins.size());points.push_back({0,float(height)});
        for(size_t i=0;i<bins.size();++i){
            auto top=float(height*(1-std::clamp(double(bins[i]),0.,1.)));
            points.push_back({float(i*step),top});points.push_back({float((i+1)*step),top});
        }
        points.push_back({float(width),float(height)});
        Shapes::Polygon shape;shape.Points().ReplaceAll(points);shape.IsHitTestVisible(false);
        auto color=colors[channel];color.A=140;shape.Fill(SolidColorBrush(color));
        target.Children().Append(shape);
    }
}
struct PlotView : std::enable_shared_from_this<PlotView> {
    std::shared_ptr<ScopeFeed> source;bool tonal=false;
    Canvas canvas;uint64_t drawn=uint64_t(-1);double width=-1,height=-1;
    void init(){
        canvas.IsHitTestVisible(false);
        auto weak=weak_from_this();
        canvas.SizeChanged([weak](auto&&,auto&&){if(auto self=weak.lock())self->draw();});
        source->watch([weak]{if(auto self=weak.lock()){self->draw();return true;}return false;});
    }
    void draw(){
        double w=canvas.ActualWidth(),h=canvas.ActualHeight();
        if(source->generation==drawn&&w==width&&h==height)return;
        drawn=source->generation;width=w;height=h;
        drawPlot(canvas,tonal?source->tonal:source->histogram,source->colors,w,h);
    }
};
std::shared_ptr<PlotView> plotView(std::shared_ptr<ScopeFeed> const& source,bool tonal){
    auto view=std::make_shared<PlotView>();view->source=source;view->tonal=tonal;view->init();return view;
}
struct WaveformView : std::enable_shared_from_this<WaveformView> {
    std::shared_ptr<ScopeFeed> source;
    Image image;Imaging::WriteableBitmap bitmap{nullptr};uint64_t installed=uint64_t(-1);
    void init(FrameworkElement const& chart){
        image.Stretch(Stretch::Fill);image.IsHitTestVisible(false);
        auto weak=weak_from_this();
        chart.SizeChanged([weak,chart](auto&&,auto&&){if(auto self=weak.lock())self->resize(chart);});
        source->watch([weak]{if(auto self=weak.lock()){self->install();return true;}return false;});
    }
    void resize(FrameworkElement const& chart){
        auto root=chart.XamlRoot();if(!root)return;double scale=root.RasterizationScale();
        source->waveformSize={int(std::lround(chart.ActualWidth()*scale)),int(std::lround(chart.ActualHeight()*scale))};
        source->refresh();
    }
    void install(){
        if(installed==source->generation)return;installed=source->generation;
        if(!source->waveform){image.Source(nullptr);bitmap=nullptr;return;}
        size_t count=0;auto bytes=capy_preview_bytes(source->waveform.get(),&count);
        if(!bytes||count!=size_t(source->waveformWidth)*size_t(source->waveformHeight)*4)return;
        if(!bitmap||bitmap.PixelWidth()!=source->waveformWidth||bitmap.PixelHeight()!=source->waveformHeight){
            bitmap=Imaging::WriteableBitmap(source->waveformWidth,source->waveformHeight);image.Source(bitmap);
        }
        uint8_t* destination=nullptr;
        check_hresult(bitmap.PixelBuffer().as<::Windows::Storage::Streams::IBufferByteAccess>()->Buffer(&destination));
        std::memcpy(destination,bytes,count);bitmap.Invalidate();
    }
};
ComboBox selector(std::shared_ptr<WorkspaceData> const& data,hstring const& id){
    ComboBox box;box.MinWidth(0);box.MinHeight(32);box.Height(32);box.HorizontalAlignment(HorizontalAlignment::Stretch);
    box.FontSize(data->textSize());box.Background(data->brush(L"input"));box.BorderThickness({0,0,0,0});box.CornerRadius({6,6,6,6});
    AutomationProperties::SetAutomationId(box,id);
    auto open=std::make_shared<bool>(false);
    box.DropDownOpened([data,open](auto&&,auto&&){if(!std::exchange(*open,true))data->popup(true);});
    box.DropDownClosed([data,open](auto&&,auto&&){if(std::exchange(*open,false))data->popup(false);});
    box.Unloaded([data,open](auto&&,auto&&){if(std::exchange(*open,false))data->popup(false);});
    return box;
}
void syncChoices(ComboBox const& box,A const& labels,int selected){
    if(box.Items().Size()!=labels.Size()){box.Items().Clear();for(auto value:labels)comboOption(box,value.GetString());}
    for(uint32_t i=0;i<labels.Size();++i)comboOptionText(box,i,labels.GetStringAt(i));
    if(box.SelectedIndex()!=selected)box.SelectedIndex(selected);
}
void sendHistogram(std::shared_ptr<WorkspaceData> const& data,J const& action){
    if(!data->updating)data->dispatch(O({{L"type",S(L"histogram")},{L"action",action}}));
}
struct Updating {
    bool& flag;bool previous;
    explicit Updating(std::shared_ptr<WorkspaceData> const& data):flag(data->updating),previous(std::exchange(flag,true)){}
    ~Updating(){flag=previous;}
};
TextBlock dim(std::shared_ptr<WorkspaceData> const& data){
    auto text=label(data,L"");text.Opacity(.7);text.TextTrimming(TextTrimming::CharacterEllipsis);text.TextWrapping(TextWrapping::NoWrap);return text;
}
struct ScopeView : std::enable_shared_from_this<ScopeView> {
    std::shared_ptr<WorkspaceData> data;bool waveform=false,tonal=false;hstring prefix;
    StackPanel root;Grid chart;ContentControl frame;ComboBox source{nullptr},channel{nullptr};CheckBox logarithmic{nullptr};
    std::array<TextBlock,2> axis{nullptr,nullptr};
    std::shared_ptr<PlotView> plot;std::shared_ptr<WaveformView> trace;
    J view()const{return object(data->state,tonal?L"tonal_histogram":waveform?L"waveform":L"histogram");}
    void init(Bindings& bindings){
        auto weak=weak_from_this();auto scopes=feed(data);
        root.Spacing(6);
        if(!tonal){
            Grid toolbar;toolbar.ColumnSpacing(6);
            for(int i=0;i<2;++i){ColumnDefinition column;column.Width({1,GridUnitType::Star});toolbar.ColumnDefinitions().Append(column);}
            source=selector(data,prefix+L"-source");channel=selector(data,prefix+L"-channel");Grid::SetColumn(channel,1);
            toolbar.Children().Append(source);toolbar.Children().Append(channel);root.Children().Append(toolbar);
            source.SelectionChanged([weak](auto&&,auto&&){if(auto self=weak.lock();self&&self->source.SelectedIndex()>=0)
                sendHistogram(self->data,O({{L"type",S(L"source")},{L"index",N(self->source.SelectedIndex())}}));});
            channel.SelectionChanged([weak](auto&&,auto&&){if(auto self=weak.lock();self&&self->channel.SelectedIndex()>=0)
                sendHistogram(self->data,O({{L"type",S(self->waveform?L"waveform_channel":L"channel")},{L"index",N(self->channel.SelectedIndex())}}));});
        }
        chart.Height(tonal?120:160);chart.Background(data->brush(L"input"));chart.CornerRadius({4,4,4,4});
        frame.Content(chart);frame.IsTabStop(true);frame.UseSystemFocusVisuals(true);
        frame.HorizontalContentAlignment(HorizontalAlignment::Stretch);frame.VerticalContentAlignment(VerticalAlignment::Stretch);
        AutomationProperties::SetAutomationId(frame,prefix+L"-chart");
        for(auto& text:axis)text=dim(data);
        if(waveform){
            trace=std::make_shared<WaveformView>();trace->source=scopes;trace->init(chart);chart.Children().Append(trace->image);
            for(int i=0;i<2;++i){
                axis[i].Margin({3,0,0,0});axis[i].IsHitTestVisible(false);axis[i].HorizontalAlignment(HorizontalAlignment::Left);
                axis[i].VerticalAlignment(i?VerticalAlignment::Top:VerticalAlignment::Bottom);chart.Children().Append(axis[i]);
            }
            root.Children().Append(frame);
        }else{
            plot=plotView(scopes,tonal);chart.Children().Append(plot->canvas);root.Children().Append(frame);
            Grid scale;for(int i=0;i<2;++i){ColumnDefinition column;column.Width({1,i?GridUnitType::Auto:GridUnitType::Star});scale.ColumnDefinitions().Append(column);}
            Grid::SetColumn(axis[1],1);scale.Children().Append(axis[0]);scale.Children().Append(axis[1]);root.Children().Append(scale);
        }
        if(!tonal){
            logarithmic=CheckBox();logarithmic.MinWidth(0);logarithmic.MinHeight(28);logarithmic.Padding({4,0,0,0});
            AutomationProperties::SetAutomationId(logarithmic,prefix+L"-log");
            auto text=label(data,L"");text.TextWrapping(TextWrapping::Wrap);logarithmic.Content(text);
            logarithmic.HorizontalContentAlignment(HorizontalAlignment::Stretch);root.Children().Append(logarithmic);
            logarithmic.Click([weak](auto&&,auto&&){if(auto self=weak.lock())
                sendHistogram(self->data,O({{L"type",S(self->waveform?L"waveform_logarithmic":L"logarithmic")},{L"enabled",B(self->logarithmic.IsChecked().Value())}}));});
        }
        root.Children().Append(ScopeFooter(data,prefix,[weak]{if(auto self=weak.lock())return self->view();return J{};},bindings));
        bindings.emplace_back([self=shared_from_this()]{self->refresh();});
    }
    void refresh(){
        Updating updating(data);auto current=view();
        if(source){
            syncChoices(source,array(current,L"sources"),int(num(current,L"source")));
            syncChoices(channel,array(current,L"channels"),int(num(current,L"channel")));
            for(auto [box,text]:{std::pair{source,data->caption(L"sampler",L"source")},std::pair{channel,data->caption(L"color",L"channel")}}){
                AutomationProperties::SetName(box,text);tooltip(box,text);
            }
        }
        auto details=str(current,L"description")+L"\n"+str(current,L"range");
        AutomationProperties::SetName(frame,str(current,L"description"));AutomationProperties::SetHelpText(frame,str(current,L"range"));tooltip(frame,details);
        auto scale=array(current,L"axis");
        for(uint32_t i=0;i<2;++i){auto text=i<scale.Size()?scale.GetStringAt(i):hstring();if(axis[i].Text()!=text)axis[i].Text(text);}
        if(logarithmic){
            auto labels=array(current,L"labels");auto text=labels.Size()?labels.GetStringAt(0):hstring();
            logarithmic.Content().as<TextBlock>().Text(text);AutomationProperties::SetName(logarithmic,text);
            logarithmic.IsChecked(flag(current,L"logarithmic"));
        }
        feed(data)->refresh();
        if(plot)plot->draw();
        if(trace)trace->install();
    }
};
}
FrameworkElement CapyScopes::ScopeFooter(std::shared_ptr<WorkspaceData> const& data,hstring const& prefix,std::function<J()> view,Bindings& bindings,
    FrameworkElement const& trailing){
    Grid footer;footer.ColumnSpacing(4);
    for(auto width:{GridUnitType::Star,GridUnitType::Auto,GridUnitType::Auto,GridUnitType::Auto}){ColumnDefinition column;column.Width({1,width});footer.ColumnDefinitions().Append(column);}
    auto status=dim(data);status.VerticalAlignment(VerticalAlignment::Center);AutomationProperties::SetAutomationId(status,prefix+L"-status");
    footer.Children().Append(status);
    if(trailing){trailing.VerticalAlignment(VerticalAlignment::Center);Grid::SetColumn(trailing,3);footer.Children().Append(trailing);}
    std::array<Primitives::ToggleButton,2> clipping{nullptr,nullptr};
    for(int i=0;i<2;++i){
        auto name=i?L"highlights":L"shadows";
        auto toggle=button<Primitives::ToggleButton>(data,L"",[]{});clipping[i]=toggle;
        toggle.Width(28);toggle.Height(28);toggle.Content(icon(hstring(L"tonal-")+name,data->theme()));
        AutomationProperties::SetAutomationId(toggle,prefix+L"-"+name);
        toggle.Click([data,name,weak=make_weak(toggle)](auto&&,auto&&){if(auto toggle=weak.get())
            sendHistogram(data,O({{L"type",S(name)},{L"enabled",B(toggle.IsChecked().Value())}}));});
        Grid::SetColumn(toggle,1+i);footer.Children().Append(toggle);
    }
    bindings.emplace_back([data,view,status,clipping]{
        auto text=str(view(),L"status");if(status.Text()!=text){status.Text(text);tooltip(status,text);AutomationProperties::SetName(status,text);}
        auto main=object(data->state,L"histogram");auto labels=array(main,L"labels");
        for(uint32_t i=0;i<2;++i){
            auto caption=i+1<labels.Size()?labels.GetStringAt(i+1):hstring();
            AutomationProperties::SetName(clipping[i],caption);tooltip(clipping[i],caption);
            bool checked=flag(main,i?L"highlights":L"shadows");if(clipping[i].IsChecked().Value()!=checked)clipping[i].IsChecked(checked);
        }
    });
    return footer;
}
FrameworkElement CapyScopes::ScopePanel(std::shared_ptr<WorkspaceData> const& data,Bindings& bindings,bool waveform){
    auto view=std::make_shared<ScopeView>();view->data=data;view->waveform=waveform;view->prefix=waveform?L"waveform":L"histogram";
    view->init(bindings);return view->root;
}
FrameworkElement CapyScopes::TonalScope(std::shared_ptr<WorkspaceData> const& data,Bindings& bindings){
    auto view=std::make_shared<ScopeView>();view->data=data;view->tonal=true;view->prefix=L"tonal";
    view->init(bindings);return view->root;
}
FrameworkElement CapyScopes::TonalPlot(std::shared_ptr<WorkspaceData> const& data,Bindings& bindings){
    auto view=plotView(feed(data),true);
    bindings.emplace_back([data,view]{feed(data)->refresh();view->draw();});
    return view->canvas;
}
