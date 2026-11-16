#pragma once
#include "WorkflowMotion.h"
#include "PanelBody.h"
#include "../SmokeCommand.h"
#include <fstream>
#include <thread>
#include <sstream>
#include <iomanip>
#include <limits>

namespace CapyTests {
struct WorkflowLayoutFixture:std::enable_shared_from_this<WorkflowLayoutFixture>{
    winrt::Microsoft::UI::Xaml::Window window{nullptr};
    std::shared_ptr<CapyUi::WorkflowPages> pages;
    std::unique_ptr<PanelBody> panel;
    CapyUi::Border outgoing{nullptr},incoming{nullptr};
    std::filesystem::path output;
    winrt::hstring fault;
    double measured=-1,desired=-1,outgoingDesired=-1,scale=0;
    bool terminal=false,animationsEnabled=false,animatedTransition=false,immediateChecked=false;
    static winrt::hstring describe(winrt::hresult_error const& error){std::wostringstream text;text<<error.message().c_str()<<L" HRESULT 0x"<<std::hex<<uint32_t(error.code());return winrt::hstring(text.str());}
    void measure(){panel->Root().Measure({320,INFINITY});measured=panel->ContentHeight(320);}
    void verify(){
        try{
            incoming.Measure({320,INFINITY});desired=incoming.DesiredSize().Height;
            if(!std::isfinite(desired)||desired<=0||desired==outgoingDesired||!std::isfinite(measured)||measured<0)throw winrt::hresult_error(E_FAIL,L"Workflow page measurement precondition was not reached");
            terminal=true;
            if(measured!=desired)fault=L"Workflow page settle retained the outgoing measured height";
            else{
                auto content=panel->Root().as<CapyUi::StackPanel>().Children().GetAt(0).as<CapyUi::StackPanel>();
                auto immediate=std::make_shared<CapyUi::WorkflowPages>();CapyUi::Border page;page.Height(233);immediate->append(page);
                content.Children().Clear();content.Children().Append(immediate->root);measured=-1;
                immediate->show(0,false,false,[self=shared_from_this()]{self->measure();});
                page.Measure({320,INFINITY});immediateChecked=measured==page.DesiredSize().Height;
                if(!immediateChecked)fault=L"Workflow immediate settle retained an invalid measured height";
            }
        }catch(winrt::hresult_error const& error){fault=describe(error);}
        window.Close();
    }
    void verifySmokeCommands(){
        using namespace CapyUi;
        Grid surface;surface.Width(944);surface.Height(660);
        StackPanel legacy;legacy.Orientation(Orientation::Horizontal);legacy.Spacing(4);
        legacy.HorizontalAlignment(HorizontalAlignment::Center);legacy.VerticalAlignment(VerticalAlignment::Bottom);
        for(auto label:{L"Test stroke",L"Test pan",L"Test backlog",L"Test pen",L"Test pen begin",L"Test pen end",L"Test filter reload",L"Test GPU loss"}){
            Button command;command.Content(box_value(label));legacy.Children().Append(command);
        }
        surface.Children().Append(legacy);
        auto arrange=[&]{surface.Measure({944,660});surface.Arrange({0,0,944,660});};arrange();
        auto before=legacy.DesiredSize();auto count=legacy.Children().Size();
        smokeCommand(surface,L"test-eraser-position",L"Test eraser",[](TextBox const&){});
        smokeCommand(surface,L"test-file-drop-arguments",L"Test file drop",[](TextBox const&){});
        arrange();
        if(legacy.Children().Size()!=count||legacy.DesiredSize()!=before)throw hresult_error(E_FAIL,L"UIA smoke commands changed the legacy toolbar geometry");
        for(uint32_t index=1;index<surface.Children().Size();++index){
            auto host=surface.Children().GetAt(index).as<Canvas>();
            if(host.IsHitTestVisible()||host.Width()!=0||host.Height()!=0)throw hresult_error(E_FAIL,L"UIA smoke command exposed a physical input surface");
            for(auto const& child:host.Children())if(child.as<Control>().IsTabStop())throw hresult_error(E_FAIL,L"UIA smoke command added a native tab stop");
        }
    }
    void start(){
        try{
            verifySmokeCommands();
            animationsEnabled=winrt::Windows::UI::ViewManagement::UISettings().AnimationsEnabled();
            pages->show(0);panel->Root().UpdateLayout();measure();
            outgoingDesired=outgoing.DesiredSize().Height;scale=panel->Root().XamlRoot().RasterizationScale();
            if(!std::isfinite(outgoingDesired)||outgoingDesired<=0||measured!=outgoingDesired||outgoing.Height()!=71||incoming.Height()!=233)throw winrt::hresult_error(E_FAIL,L"Workflow outgoing page measurement precondition was not reached");
            pages->show(1,true,false,[self=shared_from_this()]{self->measure();});
            animatedTransition=bool(pages->motion);
            if(pages->motion)pages->motion.Completed([self=shared_from_this()](auto&&,auto&&){self->verify();});
            else verify();
        }catch(winrt::hresult_error const& error){fault=describe(error);window.Close();}
    }
    void open(){
        using namespace CapyUi;
        wchar_t path[32768]{};auto length=GetEnvironmentVariableW(L"CAPY_WORKFLOW_LAYOUT_FIXTURE",path,32768);
        if(!length||length>=32768)throw winrt::hresult_error(E_INVALIDARG,L"Workflow layout output is missing");
        output=path;pages=std::make_shared<WorkflowPages>();outgoing=Border();incoming=Border();
        auto data=std::make_shared<WorkspaceData>();data->send=[](std::string){};
        panel=std::make_unique<PanelBody>(data,O({{L"id",S(L"workflow-layout")},{L"controls",A{}}}),J{},[]{},nullptr,false);
        auto content=panel->Root().as<StackPanel>().Children().GetAt(0).as<StackPanel>();
        outgoing.Height(71);incoming.Height(233);pages->append(outgoing);pages->append(incoming);content.Children().Append(pages->root);
        auto root=panel->Root();root.Width(320);root.VerticalAlignment(VerticalAlignment::Top);
        root.Loaded([self=shared_from_this()](auto&&,auto&&){self->start();});
        window=winrt::Microsoft::UI::Xaml::Window();window.Content(root);window.Activate();
    }
    int finish(winrt::hstring error=L""){
        if(!error.empty())fault=error;
        int code=terminal?(fault.empty()?0:1):2;
        bool saved=false;std::jthread writer([&]{
            std::ofstream result(output);result<<std::setprecision(std::numeric_limits<double>::max_digits10)<<"{\"process_id\":"<<GetCurrentProcessId()<<",\"terminal\":"<<(terminal?"true":"false")<<",\"exit_code\":"<<code<<",\"animations_enabled\":"<<(animationsEnabled?"true":"false")<<",\"animated_transition\":"<<(animatedTransition?"true":"false")<<",\"immediate_checked\":"<<(immediateChecked?"true":"false")<<",\"rasterization_scale\":"<<scale<<",\"requested_outgoing\":71,\"requested_incoming\":233,\"outgoing_desired\":"<<outgoingDesired<<",\"measured\":"<<measured<<",\"desired\":"<<desired<<"}\n";saved=bool(result);
            if(!fault.empty()){std::ofstream detail(std::filesystem::path(output.wstring()+L".error.txt"));detail<<winrt::to_string(fault);saved=saved&&bool(detail);}
        });writer.join();return saved?code:2;
    }
};
}
