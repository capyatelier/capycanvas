#pragma once
#include "ColorForm.h"
#include <winrt/Microsoft.UI.Xaml.Shapes.h>

namespace CapyUi {
struct ColorLibraryView : std::enable_shared_from_this<ColorLibraryView> {
    std::shared_ptr<WorkspaceData> data;
    StackPanel root;
    ComboBox palettes,swatches;
    TextBox paletteName,swatchName;
    std::shared_ptr<ColorForm> form;
    A library;
    hstring previous,paintContext;
    double paletteId=1,swatchId=0;
    bool updating=false;
    J panel()const{return object(data->model,L"color_panel");}
    J selectedPalette()const{for(auto item:library)if(num(item.GetObject(),L"id")==paletteId)return item.GetObject();return J{};}
    J selectedSwatch()const{for(auto item:array(selectedPalette(),L"swatches"))if(num(item.GetObject(),L"id")==swatchId)return item.GetObject();return J{};}
    void action(J value){data->dispatch(O({{L"type",S(L"color")},{L"action",O({{L"op",S(L"library")},{L"action",value}})}}));}
    void showSwatches(){
        updating=true;swatches.Items().Clear();int selected=-1;auto colors=array(selectedPalette(),L"swatches");
        for(uint32_t i=0;i<colors.Size();++i){auto color=colors.GetObjectAt(i);StackPanel row;row.Orientation(Orientation::Horizontal);row.Spacing(8);
            A list;list.Append(object(color,L"color"));auto preview=colorUi(O({{L"type",S(L"preview")},{L"colors",list},{L"document_space",S(str(panel(),L"rgb_space",L"Srgb"))},{L"rendition",panel().GetNamedValue(L"rendition",JsonValue::CreateNullValue())}})).GetArray().GetObjectAt(0);
            Shapes::Rectangle chip;chip.Width(20);chip.Height(20);chip.Fill(SolidColorBrush(displayColor(preview)));row.Children().Append(chip);row.Children().Append(label(data,str(color,L"name")));
            swatches.Items().Append(row);if(num(color,L"id")==swatchId)selected=i;
        }
        swatches.SelectedIndex(selected);updating=false;
    }
    void refresh(){
        auto next=array(object(object(data->state,L"colors"),L"library"),L"palettes");
        auto nextKey=next.Stringify()+str(panel(),L"rgb_space")+panel().GetNamedValue(L"rendition",JsonValue::CreateNullValue()).Stringify();
        if(nextKey!=previous){previous=nextKey;library=next;updating=true;palettes.Items().Clear();int selected=-1;
            for(uint32_t i=0;i<library.Size();++i){auto palette=library.GetObjectAt(i);palettes.Items().Append(box_value(str(palette,L"name")));if(num(palette,L"id")==paletteId)selected=i;}
            if(selected<0&&library.Size()){selected=0;paletteId=num(library.GetObjectAt(0),L"id");}
            palettes.SelectedIndex(selected);paletteName.Text(str(selectedPalette(),L"name"));updating=false;showSwatches();
        }
        paintContext=str(object(data->state,L"colors"),L"paint_slot");
        form->load(object(panel(),L"definition"),str(panel(),L"rgb_space",L"Srgb"),panel(),true);
    }
    void init(){
        root.Spacing(8);root.Width(300);auto weak=weak_from_this();form=std::make_shared<ColorForm>();
        paintContext=str(object(data->state,L"colors"),L"paint_slot");
        form->init([weak](J color){if(auto self=weak.lock()){
            self->data->dispatch(O({{L"type",S(L"color")},{L"action",O({{L"op",S(L"set_slot")},{L"slot",S(self->paintContext)},{L"color",color}})}}));
        }},L"precise-color");root.Children().Append(form->root);
        palettes.Header(box_value(L"Palette"));palettes.HorizontalAlignment(HorizontalAlignment::Stretch);root.Children().Append(palettes);
        palettes.SelectionChanged([weak](auto&&,auto&&){if(auto self=weak.lock();self&&!self->updating&&self->palettes.SelectedIndex()>=0){self->paletteId=num(self->library.GetObjectAt(self->palettes.SelectedIndex()),L"id");self->swatchId=0;self->paletteName.Text(str(self->selectedPalette(),L"name"));self->showSwatches();}});
        paletteName.Header(box_value(L"Palette name"));paletteName.MaxLength(64);root.Children().Append(paletteName);
        StackPanel manage;manage.Orientation(Orientation::Horizontal);manage.Spacing(4);
        for(auto op:{L"create_palette",L"rename_palette",L"remove_palette"}){
            auto name=std::wstring_view(op)==L"create_palette"?L"New":std::wstring_view(op)==L"rename_palette"?L"Rename":L"Delete";
            manage.Children().Append(button(data,name,[weak,op]{if(auto self=weak.lock())self->action(O({{L"op",S(op)},{L"id",N(self->paletteId)},{L"name",S(self->paletteName.Text())}}));}));
        }root.Children().Append(manage);
        swatches.Header(box_value(L"Saved colors"));swatches.HorizontalAlignment(HorizontalAlignment::Stretch);root.Children().Append(swatches);
        swatches.SelectionChanged([weak](auto&&,auto&&){if(auto self=weak.lock();self&&!self->updating&&self->swatches.SelectedIndex()>=0){auto color=array(self->selectedPalette(),L"swatches").GetObjectAt(self->swatches.SelectedIndex());self->swatchId=num(color,L"id");self->swatchName.Text(str(color,L"name"));}});
        swatchName.Header(box_value(L"Color name"));swatchName.Text(L"Color");swatchName.MaxLength(64);root.Children().Append(swatchName);
        root.Children().Append(button(data,L"Save current color",[weak]{if(auto self=weak.lock())self->action(O({{L"op",S(L"store")},{L"palette",N(self->paletteId)},{L"name",S(self->swatchName.Text())},{L"color",object(self->panel(),L"definition")}}));}));
        StackPanel edit;edit.Orientation(Orientation::Horizontal);edit.Spacing(4);
        for(auto op:{L"use",L"rename",L"remove"}){auto name=std::wstring_view(op)==L"use"?L"Use color":std::wstring_view(op)==L"rename"?L"Rename":L"Delete";
            edit.Children().Append(button(data,name,[weak,op]{if(auto self=weak.lock();self&&self->swatchId)self->action(O({{L"op",S(op)},{L"id",N(self->swatchId)},{L"name",S(self->swatchName.Text())}}));}));
        }root.Children().Append(edit);refresh();
    }
};
}
