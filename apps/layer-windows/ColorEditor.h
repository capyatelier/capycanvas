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
        form->load(object(panel(),L"definition"),str(panel(),L"rgb_space",L"Srgb"),panel(),true);
    }
    void init(){
        root.Spacing(8);root.Width(300);auto weak=weak_from_this();form=std::make_shared<ColorForm>();
        form->init([weak](J color){if(auto self=weak.lock()){
            self->data->dispatch(O({{L"type",S(L"color")},{L"action",O({{L"op",S(L"set_slot")},{L"slot",S(self->paintContext)},{L"color",color}})}}));
        }},L"precise-color");
        root.Children().Append(form->root);refresh();
    }
};
}
