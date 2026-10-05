#pragma once
#include "ColorForm.h"

namespace CapyUi {
struct ColorEditor : std::enable_shared_from_this<ColorEditor> {
    std::shared_ptr<WorkspaceData> data;
    StackPanel root;
    std::shared_ptr<ColorForm> form;
    hstring paintContext;
    J panel()const{return object(data->model,L"color_panel");}
    void refresh(){
        paintContext=str(displayColors(data->state),L"paint_slot");
        form->load(displayColors(data->state),O({{L"slot",S(paintContext)}}),false,panel());
    }
    void init(){
        root.Spacing(8);root.Width(320);auto weak=weak_from_this();form=std::make_shared<ColorForm>(data);
        form->init([weak](J color,std::optional<double> stops){if(auto self=weak.lock()){
            auto action=O({{L"op",S(stops?L"set_slot_intensity":L"set_slot")},{L"slot",S(self->paintContext)},{L"color",color}});
            if(stops)action.Insert(L"stops",N(*stops));
            self->data->dispatch(O({{L"type",S(L"color")},{L"action",action}}));
        }},L"precise-color");
        root.Children().Append(form->root);refresh();
    }
};
}
