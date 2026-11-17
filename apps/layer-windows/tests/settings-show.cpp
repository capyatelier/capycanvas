#include <coroutine>
#include <cstdint>
#include <exception>
#include <functional>
#include <iostream>
#include <initializer_list>
#include <memory>
#include <stdexcept>
#include <string>
#include <utility>

struct fire_and_forget {
    struct promise_type {
        fire_and_forget get_return_object(){return {};}
        std::suspend_never initial_suspend()noexcept{return {};}
        std::suspend_never final_suspend()noexcept{return {};}
        void return_void(){}
        void unhandled_exception(){std::terminate();}
    };
};
using hstring=std::string;
struct hresult_error {hstring message()const{return "unexpected dialog failure";}};
std::string to_string(hstring const& value){return value;}
std::wstring S(wchar_t const* value){return value;}
struct Action {std::wstring type;};
Action O(std::initializer_list<std::pair<wchar_t const*,std::wstring>> fields){return {fields.begin()->second};}
struct Data {
    unsigned closes=0;
    bool open=true;
    unsigned Size()const{return open?1u:0u;}
    void dispatch(Action const& action){if(action.type!=L"close_settings")throw std::runtime_error("Unexpected shared action");++closes;open=false;}
};
Data const& preferences(std::shared_ptr<Data> const& data){return *data;}
struct ContentDialog {
    struct State {int content=0;bool hidden=false;std::coroutine_handle<> pending{};};
    std::shared_ptr<State> state;
    ContentDialog():state(std::make_shared<State>()){}
    ContentDialog(std::nullptr_t){}
    explicit operator bool()const{return bool(state);}
    bool operator==(ContentDialog const&)const=default;
    void Content(int value){state->content=value;}
    void Content(std::nullptr_t){state->content=0;}
    int Content()const{return state->content;}
    struct Awaitable {
        std::shared_ptr<State> state;
        bool await_ready()const noexcept{return false;}
        void await_suspend(std::coroutine_handle<> continuation){state->pending=continuation;}
        void await_resume()const noexcept{}
    };
    Awaitable ShowAsync()const{return {state};}
    void Hide(){state->hidden=true;}
    void Complete(){auto pending=std::exchange(state->pending,{});if(!pending)throw std::runtime_error("No pending native show");pending.resume();}
};
struct ShortcutPage {void Apply(int){}};
struct Fixture:std::enable_shared_from_this<Fixture> {
    ContentDialog dialog{nullptr};
    std::shared_ptr<Data> data=std::make_shared<Data>();
    std::unique_ptr<ShortcutPage> shortcutPage;
    bool showing=false,closing=false,showFailed=false,stopping=false,dismissing=false,requested=false;
    uint64_t requests=0;
    int frame=17;
    void changed(){}
    void configure(){}
    void report(std::string const&){throw std::runtime_error("Unexpected show error");}
#include "settings-show.inc"
#include "settings-hide.inc"
    void RetireShared(int snapshot){
        auto model=preferences(data);
#include "settings-retire.inc"
    }
};
void require(bool passed,char const* message){if(!passed)throw std::runtime_error(message);}
unsigned failures=0;
void run(char const* name,std::function<void()> const& test){
    try{test();std::cout<<"PASS "<<name<<'\n';}
    catch(std::exception const& error){++failures;std::cout<<"FAIL "<<name<<": "<<error.what()<<'\n';}
}
int main(){
    run("native completion without Closing",[]{
        auto view=std::make_shared<Fixture>();view->show();auto shown=view->dialog;shown.Complete();
        require(view->data->closes==1,"Native Preferences completion without Closing did not close shared Preferences exactly once");
        require(!view->showing&&!view->dialog,"Completed Preferences retained its native show");
    });
    run("authorized hide preserves a shared reopen",[]{
        auto view=std::make_shared<Fixture>();view->show();auto old=view->dialog;
        view->data->open=false;view->RetireShared(0);
        require(old.state->hidden,"Shared retirement did not hide the pending native dialog");
        view->data->open=true;old.Complete();
        require(view->data->closes==0,"Completing retired Preferences closed its reopened shared request");
    });
    run("old completion preserves replacement",[]{
        auto view=std::make_shared<Fixture>();view->show();auto old=view->dialog;
        ContentDialog replacement;replacement.Content(29);view->dialog=replacement;old.Complete();
        require(view->dialog==replacement&&replacement.Content()==29&&view->showing,"Old Preferences completion retired its replacement dialog");
        require(view->data->closes==0,"Old Preferences completion closed its replacement's shared state");
    });
    run("stopping does not close shared Preferences",[]{
        auto view=std::make_shared<Fixture>();view->show();view->stopping=true;auto shown=view->dialog;shown.Complete();
        require(view->data->closes==0,"Stopping dispatched a shared Preferences close");
    });
    run("Closing does not dispatch a duplicate close",[]{
        auto view=std::make_shared<Fixture>();view->show();view->closing=true;auto shown=view->dialog;shown.Complete();
        require(view->data->closes==0,"A reconciled Preferences close dispatched twice");
    });
    run("pre-Opened hide preserves an immediate shared reopen",[]{
        auto view=std::make_shared<Fixture>();view->show();auto old=view->dialog;
        view->hide();
        require(old.state->hidden,"Pre-Opened Close did not hide the pending native dialog");
        view->data->open=true;view->data->closes=0;old.Complete();
        require(view->data->open&&view->data->closes==0,"Stale pre-Opened completion closed the immediately reopened shared request");
    });
    return failures?1:0;
}
