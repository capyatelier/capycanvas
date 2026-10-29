#include "pch.h"
#include "ScopesView.h"
#include <winrt/Microsoft.UI.Xaml.Shapes.h>
#include <winrt/Microsoft.UI.Xaml.Media.Imaging.h>
#include "BitmapUpload.h"
#include <array>
namespace CapyUi {
using Plot=std::vector<std::pair<uint32_t,std::vector<float>>>;
struct ScopeFeed : std::enable_shared_from_this<ScopeFeed> {
    std::weak_ptr<WorkspaceData> owner;
    double requested=-1;bool busy=false;uint64_t generation=0;
    std::array<winrt::Windows::UI::Color,4> colors{};
    Plot histogram,tonal;
    PreviewPacket waveform;int waveformWidth=0,waveformHeight=0;
    std::array<int,2> waveformSize{},requestedSize{},waveformRasterSize{};hstring waveformContext;
    std::vector<std::function<bool()>> views;
    std::vector<std::weak_ptr<std::function<void()>>> bindings;
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
    hstring context()const{
        auto data=owner.lock();if(!data)return L"";A value;
        value.Append(object(data->state,L"document_file").GetNamedValue(L"epoch",JsonValue::CreateNullValue()));
        value.Append(data->state.GetNamedValue(L"palette",JsonValue::CreateNullValue()));
        for(auto kind:{L"histogram",L"waveform",L"tonal_histogram"}){
            auto scope=object(data->state,kind);A options;
            for(auto field:{L"source",L"channel",L"logarithmic",L"captured_source"})options.Append(scope.GetNamedValue(field,JsonValue::CreateNullValue()));
            value.Append(options);
        }return value.Stringify();
    }
    void watch(std::function<bool()> view){views.push_back(std::move(view));}
    void refresh(){
        auto data=owner.lock();if(!data||busy||!data->query)return;
        auto revision=num(data->model,L"windows_scopes",-1);
        if(revision<0||(revision==requested&&requestedSize==waveformSize))return;
        auto dispatcher=Microsoft::UI::Dispatching::DispatcherQueue::GetForCurrentThread();if(!dispatcher)return;
        auto request=O({{L"type",S(L"scopes")}});
        if(waveformSize[0]>0&&waveformSize[1]>0){A size;size.Append(N(waveformSize[0]));size.Append(N(waveformSize[1]));request.Insert(L"size",size);}
        busy=true;requested=revision;requestedSize=waveformSize;auto expected=context();auto size=waveformSize;
        bool queued=data->query(CanvasQueryKind::Workspace,to_string(request.Stringify()),[dispatcher,weak=weak_from_this(),expected,size](PreviewPacket packet){
            dispatcher.TryEnqueue([weak,packet=std::move(packet),expected,size]{if(auto self=weak.lock())self->receive(packet,expected,size);});
        });
        if(!queued){busy=false;requested=-1;}
    }
    void receive(PreviewPacket const& packet,hstring const& expected,std::array<int,2> size){
        busy=false;
        if(expected!=context()||size!=waveformSize){requested=-1;refresh();return;}
        if(packet)try{
            auto result=object(J::Parse(to_hstring(NativeString(capy_preview_metadata(packet.get())).get())),L"result");
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
                waveformContext=expected;waveformRasterSize=size;++generation;
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
void bindScope(std::shared_ptr<WorkspaceData> const& data,Bindings& bindings,std::function<void()> update){
    auto binding=std::make_shared<std::function<void()>>(std::move(update));
    feed(data)->bindings.push_back(binding);bindings.emplace_back([binding]{(*binding)();});
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
    Image image;std::shared_ptr<BitmapUpload> upload=BitmapUpload::Create();uint64_t installed=uint64_t(-1);hstring context;
    ~WaveformView(){upload->Close();}
    void init(FrameworkElement const& chart){
        image.Stretch(Stretch::Fill);image.IsHitTestVisible(false);
        auto weak=weak_from_this();
        chart.SizeChanged([weak,chart](auto&&,auto&&){if(auto self=weak.lock())self->resize(chart);});
        source->watch([weak]{if(auto self=weak.lock()){self->install();return true;}return false;});
    }
    void resize(FrameworkElement const& chart){
        auto root=chart.XamlRoot();if(!root)return;double scale=root.RasterizationScale();
        std::array<int,2> size{int(std::lround(chart.ActualWidth()*scale)),int(std::lround(chart.ActualHeight()*scale))};
        if(size!=source->waveformSize){source->waveformSize=size;upload->Clear();image.Source(nullptr);installed=uint64_t(-1);}
        source->refresh();
    }
    void install(){
        auto current=source->context();
        if(context!=current){context=current;upload->Clear();image.Source(nullptr);installed=uint64_t(-1);}
        if(installed==source->generation||source->waveformContext!=current||source->waveformRasterSize!=source->waveformSize)return;
        installed=source->generation;
        if(!source->waveform){upload->Clear();image.Source(nullptr);return;}
        auto weak=weak_from_this();auto size=source->waveformSize;
        upload->Submit({BitmapUpload::ReadPacket(source->waveform),0,size_t(source->waveformWidth)*source->waveformHeight*4,
            source->waveformWidth,source->waveformHeight,false,true,
            [weak,current,size]{auto self=weak.lock();return self&&self->source->context()==current&&self->source->waveformSize==size;},
            [weak](bool accepted){if(auto self=weak.lock();self&&accepted)self->image.Source(self->upload->Image());}});
    }
};
ComboBox selector(std::shared_ptr<WorkspaceData> const& data,hstring const& id){
    ComboBox box;box.MinWidth(0);box.MinHeight(data->controlHeight());box.Height(data->controlHeight());box.Padding({6,0,0,0});box.HorizontalAlignment(HorizontalAlignment::Stretch);
    box.FontSize(data->textSize());box.FontWeight(Windows::UI::Text::FontWeights::Bold());choiceColors(data,box);
    AutomationProperties::SetAutomationId(box,id);
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
    auto text=label(data,L"");text.Opacity(.55);text.TextTrimming(TextTrimming::CharacterEllipsis);text.TextWrapping(TextWrapping::NoWrap);return text;
}
struct ScopeView : std::enable_shared_from_this<ScopeView> {
    std::shared_ptr<WorkspaceData> data;bool waveform=false,tonal=false,flexible=false;hstring prefix;
    Grid root,chart,toolbar{nullptr},graph{nullptr},scale{nullptr};ContentControl frame;FrameworkElement footer{nullptr};ComboBox source{nullptr},channel{nullptr};CheckBox logarithmic{nullptr};
    std::array<TextBlock,2> axis{nullptr,nullptr};
    std::shared_ptr<PlotView> plot;std::shared_ptr<WaveformView> trace;
    J view()const{return object(data->state,tonal?L"tonal_histogram":waveform?L"waveform":L"histogram");}
    double naturalHeight()const{return chart.MaxHeight()+toolbar.DesiredSize().Height+logarithmic.DesiredSize().Height+footer.DesiredSize().Height
        +(scale?scale.DesiredSize().Height+graph.RowSpacing():0)+3*root.RowSpacing();}
    void init(Bindings& bindings){
        auto weak=weak_from_this();auto scopes=feed(data);
        root.RowSpacing(data->panelMetric(L"control_spacing"));root.UseLayoutRounding(false);
        for(int i=0;i<(tonal?2:4);++i){RowDefinition row;row.Height({1,i==(tonal?0:1)?GridUnitType::Star:GridUnitType::Auto});root.RowDefinitions().Append(row);}
        if(!tonal){
            toolbar=Grid();toolbar.ColumnSpacing(data->panelMetric(L"control_spacing"));
            for(int i=0;i<2;++i){ColumnDefinition column;column.Width({1,GridUnitType::Star});toolbar.ColumnDefinitions().Append(column);}
            source=selector(data,prefix+L"-source");channel=selector(data,prefix+L"-channel");Grid::SetColumn(channel,1);
            toolbar.Children().Append(source);toolbar.Children().Append(channel);root.Children().Append(toolbar);
            source.SelectionChanged([weak](auto&&,auto&&){if(auto self=weak.lock();self&&self->source.SelectedIndex()>=0)
                sendHistogram(self->data,O({{L"type",S(L"source")},{L"index",N(self->source.SelectedIndex())}}));});
            channel.SelectionChanged([weak](auto&&,auto&&){if(auto self=weak.lock();self&&self->channel.SelectedIndex()>=0)
                sendHistogram(self->data,O({{L"type",S(self->waveform?L"waveform_channel":L"channel")},{L"index",N(self->channel.SelectedIndex())}}));});
        }
        if(flexible)chart.MaxHeight(160);else chart.Height(tonal?120:160);
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
            Grid::SetRow(frame,1);root.Children().Append(frame);
        }else{
            plot=plotView(scopes,tonal);chart.Children().Append(plot->canvas);graph=Grid();graph.RowSpacing(data->panelMetric(L"control_spacing"));graph.Children().Append(frame);
            for(int i=0;i<2;++i){RowDefinition row;row.Height({1,i?GridUnitType::Auto:GridUnitType::Star});graph.RowDefinitions().Append(row);}
            scale=Grid();for(int i=0;i<2;++i){ColumnDefinition column;column.Width({1,i?GridUnitType::Auto:GridUnitType::Star});scale.ColumnDefinitions().Append(column);}
            Grid::SetColumn(axis[1],1);scale.Children().Append(axis[0]);scale.Children().Append(axis[1]);Grid::SetRow(scale,1);graph.Children().Append(scale);
            Grid::SetRow(graph,tonal?0:1);root.Children().Append(graph);
        }
        if(!tonal){
            auto text=label(data,L"");text.TextWrapping(TextWrapping::Wrap);logarithmic=compactCheck(data,text);
            AutomationProperties::SetAutomationId(logarithmic,prefix+L"-log");
            logarithmic.HorizontalContentAlignment(HorizontalAlignment::Stretch);Grid::SetRow(logarithmic,2);root.Children().Append(logarithmic);
            logarithmic.Click([weak](auto&&,auto&&){if(auto self=weak.lock())
                sendHistogram(self->data,O({{L"type",S(self->waveform?L"waveform_logarithmic":L"logarithmic")},{L"enabled",B(self->logarithmic.IsChecked().Value())}}));});
        }
        footer=ScopeFooter(data,prefix,[weak]{if(auto self=weak.lock())return self->view();return J{};},bindings);
        Grid::SetRow(footer,tonal?1:3);root.Children().Append(footer);
        bindScope(data,bindings,[self=shared_from_this()]{self->refresh();});
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
        auto axisLabels=array(current,L"axis");
        for(uint32_t i=0;i<2;++i){auto text=i<axisLabels.Size()?axisLabels.GetStringAt(i):hstring();if(axis[i].Text()!=text)axis[i].Text(text);}
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
    Grid footer;footer.ColumnSpacing(data->panelMetric(L"control_spacing"));
    ColumnDefinition statusColumn;statusColumn.Width({1,GridUnitType::Star});footer.ColumnDefinitions().Append(statusColumn);
    auto status=dim(data);status.VerticalAlignment(VerticalAlignment::Center);AutomationProperties::SetAutomationId(status,prefix+L"-status");footer.Children().Append(status);
    auto append=[&footer](FrameworkElement const& control){ColumnDefinition column;column.Width({1,GridUnitType::Auto});Grid::SetColumn(control,int(footer.ColumnDefinitions().Size()));footer.ColumnDefinitions().Append(column);footer.Children().Append(control);};
    StackPanel clippingRow;clippingRow.Orientation(Orientation::Horizontal);
    std::array<Primitives::ToggleButton,2> clipping{nullptr,nullptr};
    for(int i=0;i<2;++i){
        auto name=i?L"highlights":L"shadows";
        auto toggle=button<Primitives::ToggleButton>(data,L"",[]{});clipping[i]=toggle;
        toggle.Width(data->panelMetric(L"control_height"));toggle.Height(data->panelMetric(L"control_height"));toggle.Content(icon(hstring(L"tonal-")+name,data->theme()));
        AutomationProperties::SetAutomationId(toggle,prefix+L"-"+name);
        toggle.Click([data,name,weak=make_weak(toggle)](auto&&,auto&&){if(auto toggle=weak.get())
            sendHistogram(data,O({{L"type",S(name)},{L"enabled",B(toggle.IsChecked().Value())}}));});
        clippingRow.Children().Append(toggle);
    }
    append(clippingRow);if(trailing){trailing.VerticalAlignment(VerticalAlignment::Center);append(trailing);}
    bindScope(data,bindings,[data,view,status,clipping]{
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
FrameworkElement CapyScopes::ScopePanel(std::shared_ptr<WorkspaceData> const& data,Bindings& bindings,bool waveform,std::function<double(double)>* naturalHeight){
    auto view=std::make_shared<ScopeView>();view->data=data;view->waveform=waveform;view->flexible=naturalHeight!=nullptr;view->prefix=waveform?L"waveform":L"histogram";
    view->init(bindings);if(naturalHeight)*naturalHeight=[weak=std::weak_ptr(view)](double width){if(auto view=weak.lock()){view->root.Measure({float(width),INFINITY});return view->naturalHeight();}return -1.;};return view->root;
}
FrameworkElement CapyScopes::TonalScope(std::shared_ptr<WorkspaceData> const& data,Bindings& bindings){
    auto view=std::make_shared<ScopeView>();view->data=data;view->tonal=true;view->prefix=L"tonal";
    view->init(bindings);return view->root;
}
FrameworkElement CapyScopes::TonalPlot(std::shared_ptr<WorkspaceData> const& data,Bindings& bindings){
    auto view=plotView(feed(data),true);
    bindScope(data,bindings,[data,view]{feed(data)->refresh();view->draw();});
    return view->canvas;
}
void CapyScopes::Refresh(std::shared_ptr<WorkspaceData> const& data){
    if(!data->scopes)return;
    Updating updating(data);
    std::erase_if(data->scopes->bindings,[](auto const& weak){
        auto binding=weak.lock();if(!binding)return true;(*binding)();return false;
    });
    data->scopes->refresh();
}
