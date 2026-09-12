#include "pch.h"
#include "StatsView.h"
#include "WorkspaceQuery.h"
#include <chrono>
#include <winrt/Microsoft.UI.Xaml.Shapes.h>

using namespace CapyUi;
namespace {
struct StatsView:std::enable_shared_from_this<StatsView>{
    std::shared_ptr<WorkspaceData> data;
    StackPanel root;
    Canvas chart;
    Microsoft::UI::Xaml::Shapes::Polyline samples;
    Microsoft::UI::Xaml::Shapes::Line budget;
    Microsoft::UI::Dispatching::DispatcherQueueTimer timer{nullptr};
    std::vector<TextBlock> values;
    J model;
    bool busy=false;
    uint64_t generation=0;
    ~StatsView(){if(timer)timer.Stop();}
    bool visible()const{
        // An empty stack has zero natural height until its first query arrives.
        if(!root.IsLoaded()||root.ActualWidth()<=0)return false;
        DependencyObject item=root;
        while(item){
            if(auto element=item.try_as<UIElement>();element&&element.Visibility()!=Visibility::Visible)return false;
            item=VisualTreeHelper::GetParent(item);
        }
        return true;
    }
    void init(){
        root.Spacing(6);
        AutomationProperties::SetAutomationId(root,L"renderer-stats");
        chart.Height(46);chart.IsHitTestVisible(false);
        AutomationProperties::SetAutomationId(chart,L"renderer-stats-chart");
        samples.Stroke(data->brush(L"text"));samples.StrokeThickness(1);
        budget.Stroke(data->brush(L"settings_secondary"));budget.StrokeThickness(1);
        chart.Children().Append(budget);chart.Children().Append(samples);
        chart.SizeChanged([weak=weak_from_this()](auto&&,auto&&){if(auto self=weak.lock())self->draw();});
        timer=root.DispatcherQueue().CreateTimer();timer.Interval(std::chrono::milliseconds(200));
        timer.Tick([weak=weak_from_this()](auto&&,auto&&){if(auto self=weak.lock())self->refresh();});
        root.Loaded([weak=weak_from_this()](auto&&,auto&&){if(auto self=weak.lock()){self->timer.Start();self->refresh();}});
        root.Unloaded([weak=weak_from_this()](auto&&,auto&&){if(auto self=weak.lock()){self->timer.Stop();++self->generation;}});
    }
    void refresh(){
        if(busy||!visible())return;
        busy=true;auto requested=generation;
        if(!QueryWorkspace(data->query,O({{L"type",S(L"renderer_stats")}}),
            [weak=weak_from_this(),requested](J reply){
                if(auto self=weak.lock()){
                    self->busy=false;
                    if(requested!=self->generation||!self->visible())return;
                    auto model=object(reply,L"result");
                    if(model.Size())self->apply(model);
                }
            }))busy=false;
    }
    void apply(J const& next){
        model=next;auto rows=array(model,L"rows");
        if(values.size()!=rows.Size()){
            root.Children().Clear();values.clear();
            for(uint32_t i=0;i<rows.Size();i++){
                auto row=rows.GetObjectAt(i);Grid line;
                ColumnDefinition caption;caption.Width({1,GridUnitType::Star});line.ColumnDefinitions().Append(caption);
                ColumnDefinition reading;reading.Width({1,GridUnitType::Auto});line.ColumnDefinitions().Append(reading);
                line.ColumnSpacing(6);
                auto text=label(data,str(row,L"label"));line.Children().Append(text);
                auto value=label(data,L"");Grid::SetColumn(value,1);line.Children().Append(value);values.push_back(value);
                AutomationProperties::SetAutomationId(value,L"renderer-stat-"+to_hstring(i));
                ToolTipService::SetToolTip(line,box_value(str(row,L"description")));
                AutomationProperties::SetHelpText(value,str(row,L"description"));
                root.Children().Append(line);
                if(i+1==uint32_t(num(model,L"chart_after_rows"))){
                    root.Children().Append(chart);
                    AutomationProperties::SetName(chart,str(model,L"chart_label"));
                }
            }
        }
        for(uint32_t i=0;i<rows.Size();i++)values[i].Text(str(rows.GetObjectAt(i),L"value"));
        draw();
    }
    void draw(){
        auto readings=array(model,L"samples");double limit=num(model,L"budget_ms",1000./120.);
        double maximum=limit;
        for(auto value:readings)maximum=std::max(maximum,value.GetNumber());
        maximum=std::max(0.001,maximum*1.1);double width=chart.ActualWidth(),height=chart.Height();
        auto points=PointCollection();
        for(uint32_t i=0;i<readings.Size();i++)
            points.Append({float(i*width/119.),float(height*(1-readings.GetNumberAt(i)/maximum))});
        samples.Points(points);
        budget.X1(0);budget.X2(width);budget.Y1(height*(1-limit/maximum));budget.Y2(budget.Y1());
    }
};
}
FrameworkElement StatsPanel(std::shared_ptr<WorkspaceData> const& data,Bindings& bindings){
    auto view=std::make_shared<StatsView>();view->data=data;view->init();
    bindings.emplace_back([view]{view->refresh();});return view->root;
}
