import {bindCopy,liveCopy} from "./localization.js";
import { createWorkspaceSwitcher } from "./workspace-switcher.js";
export function createWorkspaceManager({ app, store, applyChange, element, button, icon, dropdown, message }) {
  let view, inline;
  const copy=liveCopy(app,"catalog").native_copy.header,common=liveCopy(app,"bootstrap_view").common;
  const localizedButton=(read,run,classes)=>bindCopy(button("",run,classes),read);
  const dialog = element("dialog", "workspace-manager"), formDialog = element("dialog", "workspace-form");
  const heading = element("h2"), header = element("header", "dialog-header");
  bindCopy(heading,()=>view?.title??"");
  heading.id = "workspace-manager-title"; dialog.setAttribute("aria-labelledby", heading.id);
  const add = button("", () => send({ type: "form", action: view?.page === "this_workspace" ? {type:"new_toolbar",value:null} : { type: "new" } }), "workspace-add");
  const workspaceAdd = button("", () => send({type:"form",action:{type:"new"}}), "workspace-add");
  workspaceAdd.append(icon("plus")); bindCopy(workspaceAdd,()=>copy.new_workspace,"ariaLabel");bindCopy(workspaceAdd,()=>copy.new_workspace,"title");
  const remove = button("",()=>{const action=view?.rows.find(row=>row.id===view.selected)?.options.find(item=>item.action.type==="delete");if(action?.enabled)send({type:"action",action:action.action});},"workspace-delete");
  remove.append(icon("delete"));bindCopy(remove,()=>common.delete,"ariaLabel");bindCopy(remove,()=>common.delete,"title");
  const rowActions=element("div","workspace-row-actions");rowActions.append(workspaceAdd,remove);
  const close = button("", cancel, "dialog-close"); bindCopy(close,()=>common.close,"ariaLabel");
  close.append(element("span"));
  dialog.tabIndex = -1;
  header.append(heading, close);
  const intro = element("p", "workspace-intro");bindCopy(intro,()=>view?.intro??"");
  const list = element("div", "workspace-list"); list.setAttribute("role", "listbox");
  const search=element("input","workspace-search");search.type="search";search.id="workspace-manager-search";
  bindCopy(search,()=>copy.search,"placeholder");bindCopy(search,()=>copy.search,"ariaLabel");
  search.addEventListener("input",()=>send({type:"search",query:search.value}));
  const listActions=element("div","workspace-list-actions");listActions.append(search,add);
  const tabs=element("div","workspace-manager-tabs");
  for(const [page,label]of [["this_workspace",()=>copy.this_workspace],["toolbar_library",()=>copy.saved_toolbars]]){
    const tab=localizedButton(label,()=>send({type:"open",page}));tab.dataset.page=page;tabs.append(tab);
  }
  const details=element("section","workspace-details");
  const error = element("p", "workspace-error"), footer = element("footer");
  const primary = localizedButton(()=>view?.primary??"", () => send({ type: "confirm" }), "suggested-action");
  const retry = localizedButton(()=>copy.retry, () => send({type:"retry"})); retry.hidden = true;
  footer.append(localizedButton(()=>common.cancel,cancel), retry, primary); dialog.append(header, intro, tabs, listActions, list, rowActions, details, error, footer);
  document.body.append(dialog, formDialog);
  const switcher = createWorkspaceSwitcher({app,dialog, list, element, button, icon, send, getView:() => view,
    redraw:() => { lastView = null; render(); }});
  const recoveryButton = localizedButton(()=>view?.error??"", () => { dismissedError = null; showRecovery(view.error); }, "workspace-recovery");
  recoveryButton.hidden = true; switcher.root.after(recoveryButton);
  let switcherRevision;
  let lastView, pageKey, rowsKey, formKey, timer, formName, formDescription, formChoice, formError, formSubmit, previousFocus, observePending = false;
  const expectedCloses = new WeakMap();
  const channel = new BroadcastChannel("capycanvas.workspace.windows");
  channel.addEventListener("message", e => {
    if (e.data?.switcher) send({type:"refresh_switcher"});
    if (e.data?.focus === view?.id) { window.focus(); document.title = `CapyCanvas — ${view.name}`; }
  });
  function send(input) {
    try {
      if(inline && !["submit","cancel","dismiss","search","refresh_switcher","edit_switcher","resume","suspend"].includes(input.type)) {
        if(!finishName())return;
      }
      applyChange(app.workspace_input(JSON.stringify(input))); render(); tick();
    }
    catch (e) { message(e); }
  }
  function cancel() { switcher.cancel(); send({ type: formDialog.open?"cancel":"dismiss" }); }
  function finishName(cancel=false) {
    if(!inline||inline.submitted)return true;
    const name=inline.input.value.trim();
    if(!cancel&&app.workspace_name_validation(name)){inline.input.setAttribute("aria-invalid","true");return false;}
    inline.submitted=true;send(cancel?{type:"cancel"}:{type:"submit",name,description:null,choice:null});return true;
  }
  for (const node of [dialog, formDialog]) {
    node.addEventListener("cancel", e => { e.preventDefault();switcher.cancel();send({type:node===dialog?"dismiss":"cancel"}); });
    node.addEventListener("close", () => {
      const expected = expectedCloses.get(node) || 0;
      if (expected) expectedCloses.set(node, expected - 1);
      else if (node === formDialog ? view?.prompt : view?.page) cancel();
    });
    node.addEventListener("click", e => { if (e.target === node) { const r = node.getBoundingClientRect(); if (e.clientX < r.left || e.clientX > r.right || e.clientY < r.top || e.clientY > r.bottom) cancel(); } });
  }
  function show(node, open) {
    if (open && !node.open) { if (!dialog.open && !formDialog.open) previousFocus = document.activeElement; node.showModal(); if (node === dialog) node.focus({preventScroll:true}); }
    if (!open && node.open) { expectedCloses.set(node,(expectedCloses.get(node)||0)+1); node.close(); if (!dialog.open && !formDialog.open) previousFocus?.focus?.(); }
  }
  function render() {
    const json = app.workspace_view(); if (json === lastView) return; lastView = json; view = JSON.parse(json);
    if (!view) return;
    bindCopy(recoveryButton,()=>view?.error??"");recoveryButton.hidden = !view.error;
    if (!view.error) { dismissedError = null; if (recovery?.open) recovery.close(); }
    switcher.render(view);
    if (switcherRevision != null && switcherRevision !== view.switcher_revision) channel.postMessage({switcher:true});
    switcherRevision = view.switcher_revision;
    if (view.focus_window) { channel.postMessage({ focus: view.focus_window.id }); message(copy.owned_elsewhere); }
    if (view.page !== pageKey) { pageKey = view.page; list.scrollTop = 0; }
    bindCopy(heading,()=>view?.title??"");bindCopy(intro,()=>view?.intro??"");intro.hidden = !view.intro;
    const toolbar=["this_workspace","toolbar_library"].includes(view.page);
    const renaming=view.page==="workspaces"&&view.prompt_action?.type==="rename";
    dialog.dataset.toolbar=String(toolbar);tabs.hidden=!toolbar;details.hidden=!toolbar;
    for(const tab of tabs.children){tab.disabled=view.busy;tab.setAttribute("aria-pressed",String(tab.dataset.page===view.page));}
    search.hidden=!toolbar;
    if(search.value!==(view.query??""))search.value=view.query??"";search.disabled=view.busy;
    add.hidden = view.page!=="this_workspace"; add.disabled = view.busy || view.switcher_busy;
    bindCopy(add,()=>view?.page==="this_workspace"?copy.new_toolbar:copy.new_workspace);
    listActions.hidden=search.hidden&&add.hidden;
    rowActions.hidden=view.page!=="workspaces";
    workspaceAdd.disabled=(view.busy&&!renaming)||view.switcher_busy;
    const deleteAction=view.rows.find(row=>row.id===view.selected)?.options.find(item=>item.action.type==="delete");
    remove.disabled=(view.busy&&!renaming)||!deleteAction?.enabled;
    bindCopy(add,()=>view?.page==="this_workspace"?copy.new_toolbar:copy.new_workspace,"ariaLabel");
    bindCopy(primary,()=>view?.primary??""); primary.disabled = (view.busy&&!renaming) || view.switcher_busy || !view.enabled;
    retry.hidden = !view.retry; retry.disabled = view.busy;
    bindCopy(error,()=>view?.switcher_error||view?.error||"");error.hidden = !error.textContent;
    const newRowsKey = JSON.stringify([view.page, view.rows.map(({id,subtitle})=>({id,subtitle:!!subtitle})), view.id]);
    if (newRowsKey !== rowsKey && !switcher.dragging()) {
      const focusedId = document.activeElement.closest?.(".workspace-row")?.dataset.id;
      switcher.closeMenu();
      const scroll = list.scrollTop;
      rowsKey = newRowsKey; list.replaceChildren();
      for (const row of view.rows) {
        const container = element("div", "workspace-row"), select = view.page==="workspaces"?element("div","workspace-choice"):button("",()=>send({type:"select",id:row.id}),"workspace-choice");
        if(view.page==="workspaces"){
          select.tabIndex=0;
          select.addEventListener("click",event=>{if(!event.target.closest("input")&&(!view.busy||view.prompt_action?.type==="rename"))send({type:"select",id:row.id});});
          select.addEventListener("keydown",event=>{if(event.target.closest("input"))return;if(["Enter"," "].includes(event.key)){event.preventDefault();if(!view.busy||view.prompt_action?.type==="rename")send({type:"select",id:row.id});}});
          const rename=()=>{const action=view.rows.find(item=>item.id===row.id)?.options.find(item=>item.action.type==="rename");if(action?.enabled)send({type:"action",action:action.action});};
          select.addEventListener("dblclick",event=>{if(event.target.closest(".workspace-row-title"))rename();});
          select.addEventListener("keydown",event=>{if(event.key==="F2"&&!event.target.closest("input")){event.preventDefault();rename();}});
        }
        select.dataset.id = row.id; select.setAttribute("role", "option");
        const title=element("span","workspace-row-title");bindCopy(title,()=>view?.rows.find(item=>item.id===row.id)?.title??row.title);
        if(view.page==="workspaces"){const name=element("span","workspace-name-slot");name.append(title);select.append(name);}else select.append(title);
        if(row.subtitle){const subtitle=element("span","workspace-row-subtitle");bindCopy(subtitle,()=>view?.rows.find(item=>item.id===row.id)?.subtitle??"");select.append(subtitle);}
        container.append(select);
        if (view.page === "workspaces") switcher.decorate(container, select, row, view);
        list.append(container);
      }
      if (focusedId) [...list.querySelectorAll('.workspace-choice')].find(node=>node.dataset.id===focusedId)?.focus({preventScroll:true});
      list.scrollTop = scroll;
    }
    for (const row of list.querySelectorAll(".workspace-choice")) { const title=row.querySelector(".workspace-row-title");if(title)bindCopy(title,()=>view?.rows.find(item=>item.id===row.dataset.id)?.title??"");const subtitle=row.querySelector(".workspace-row-subtitle");if(subtitle)bindCopy(subtitle,()=>view?.rows.find(item=>item.id===row.dataset.id)?.subtitle??"");row.setAttribute("aria-selected", String(row.dataset.id === view.selected)); row.parentElement.dataset.selected = String(row.dataset.id === view.selected); row.disabled = view.busy&&!renaming;row.setAttribute("aria-disabled",String(view.busy&&!renaming)); }
    if(toolbar){
      const detail=view.details,key=JSON.stringify([detail,view.busy]);
      if(details.dataset.key!==key){details.dataset.key=key;details.replaceChildren();if(detail){
        details.append(element("h3","",detail.title),element("p","",detail.description));
        for(const offered of detail.actions??[]){const run=button(offered.label,()=>send({type:"action",action:offered.action}),offered.primary?"suggested-action":"");run.dataset.action=offered.action.type;run.disabled=view.busy||!offered.enabled;details.append(run);}
      }}
    }
    const renameId=view.page==="workspaces"&&view.prompt_action?.type==="rename"?view.prompt_action.id??view.prompt_action.value:null;
    if(inline&&inline.id!==renameId){inline.input.replaceWith(inline.title);inline=null;}
    if(renameId&&!inline){
      const row=[...list.children].find(row=>row.dataset.id===renameId),title=row?.querySelector(".workspace-row-title");
      if(title){
        const input=element("input","workspace-name-entry");input.value=view.prompt.name;bindCopy(input,()=>common.name,"ariaLabel");
        inline={id:renameId,input,title,submitted:false};title.replaceWith(input);
        input.addEventListener("keydown",event=>{event.stopPropagation();if(event.key==="Enter"||event.key==="Escape"){event.preventDefault();finishName(event.key==="Escape");}});
        input.addEventListener("blur",()=>setTimeout(()=>{if(inline?.input===input)finishName();},0));
        input.focus({preventScroll:true});input.select();
      }
    }
    if(inline){if(!view.busy)inline.submitted=false;inline.input.setAttribute("aria-invalid",String(!!view.error));}
    const form = renameId?null:view.prompt;
    const key = form ? JSON.stringify([view.prompt_action, form.name!=null, form.description!=null, (form.choices??[]).map(choice=>choice.id)]) : null;
    if (key !== formKey) {
      formKey = key; formDialog.replaceChildren(); formName = null;formDescription=null;formChoice=null;
      if (form) {
        const heading = element("h2");bindCopy(heading,()=>JSON.parse(app.workspace_view()).prompt?.title??""); heading.id = "workspace-form-title";
        formDialog.setAttribute("aria-labelledby", heading.id); formDialog.append(heading);
        if(form.message){const message=element("p");bindCopy(message,()=>JSON.parse(app.workspace_view()).prompt?.message??"");formDialog.append(message);}
        if (form.name != null) {
          const label = element("label");bindCopy(label,()=>common.name); formName = element("input"); formName.value = form.name;formName.id="workspace-item-name"; formName.maxLength = 100;formName.addEventListener("input",validatePrompt);
          bindCopy(formName,()=>common.name,"ariaLabel"); label.append(formName); formDialog.append(label);
        }
        if(form.description!=null){
          const label=element("label");bindCopy(label,()=>copy.description);formDescription=element("textarea");formDescription.id="workspace-item-description";formDescription.value=form.description;
          bindCopy(formDescription,()=>copy.description,"ariaLabel");label.append(formDescription);formDialog.append(label);
        }
        if(form.choices?.length){
          const label=element("label");bindCopy(label,()=>view?.prompt?.choice_label??"");formChoice=dropdown(()=>view?.prompt?.choice_label??'');formChoice.id="workspace-item-choice";
          formChoice.update({choices:form.choices.map(choice=>({value:choice.id,label:()=>view?.prompt?.choices.find(item=>item.id===choice.id)?.label??choice.label})),value:form.selected??form.choices[0].id});label.append(formChoice);formDialog.append(label);
        }
        formError = element("p", "workspace-error"); const footer = element("footer");
        formSubmit = button(form.confirm, () => {if(formKey!==key||!view?.prompt||!validatePrompt())return;send({ type: "submit", name: formName?.value || "",description:formDescription?.value??null,choice:formChoice?.value??null });}, "suggested-action");
        formName?.addEventListener("keydown",event=>{if(event.key==="Enter"){event.preventDefault();if(validatePrompt())formSubmit.click();}});
        if (form.destructive) formSubmit.classList.add("destructive-action");
        footer.append(localizedButton(()=>common.cancel,cancel), formSubmit); formDialog.append(formError, footer);
      }
    }
    if (form) { validatePrompt();bindCopy(formSubmit,()=>view?.retry&&view?.prompt_action?.type!=="save_as_new"?copy.retry:view?.prompt?.confirm??""); }
    list.style.flex=view.page==="workspaces"?"0 1 auto":"";
    list.style.height=view.page==="workspaces"?`${Math.min(7,view.rows.length)*56}px`:"";
    show(dialog, !!view.page); show(formDialog, !!form);
    if(dialog.open&&!form&&!inline&&(!dialog.contains(document.activeElement)||document.activeElement===dialog))list.querySelector('[aria-selected="true"]')?.focus({preventScroll:true});
    if (!view.page && !form && view.error) showRecovery(view.error);
    switcher.root.dispatchEvent(new Event("workspace-view-changed"));
  }
  function validatePrompt(){
    if(!formSubmit)return false;
    const validation=()=>formName?app.workspace_name_validation(formName.value):null;
    bindCopy(formError,()=>validation()??view?.error??"");
    formSubmit.disabled=!!(view?.busy||view?.switcher_busy||validation());
    return !formSubmit.disabled;
  }
  let recovery, recoveryText, dismissedError;
  function showRecovery(text) {
    if (text === dismissedError) return;
    if (!recovery) {
      recovery = element("dialog", "workspace-form"); bindCopy(recovery,()=>copy.workspaces,"ariaLabel");
      recoveryText = element("p"); const footer = element("footer");
      const hide = () => { recovery.close(); };
      recovery.addEventListener("cancel", e => { e.preventDefault(); dismissedError = view.error; hide(); send({type:"resume"}); });
      footer.append(localizedButton(()=>common.keep_open, () => { dismissedError = view.error; hide(); send({type:"resume"}); }), localizedButton(()=>copy.retry, () => { dismissedError = null; hide(); send({ type: "retry" }); }),
        localizedButton(()=>copy.save_as_new_workspace, () => { hide(); send({ type: "form", action: { type: "save_as_new" } }); }, "suggested-action"));
      const title=element("h2");bindCopy(title,()=>copy.workspaces);recovery.append(title,recoveryText,footer); document.body.append(recovery);
    }
    bindCopy(recoveryText,()=>view?.error??"");if (!recovery.open) recovery.showModal();
  }
  let startupReady = false;
  let resolveReady;
  const ready = new Promise(resolve => { resolveReady = resolve; });
  function tick() {
    if (observePending) { observePending = false; app.workspace_observe(); }
    applyChange(app.workspace_tick()); render();
    if (!startupReady && view?.ready && !view.busy) {
      startupReady = true;
      performance.mark("capy.startup.workspace");
      resolveReady();
    }
  }
  let wakeTimer;
  function wake() {
    // JsFuture must observe the settled reply before the controller polls it.
    // A task also avoids reentering a borrowed Wasm session and coalesces replies.
    if (wakeTimer != null) return;
    wakeTimer = setTimeout(() => { wakeTimer = null; tick(); }, 0);
  }
  // Reload replaces this document in the same browsing context. Reusing its
  // fenced identity avoids treating the old document's short lease as another
  // window. A new navigation/tab always receives an independent identity, even
  // when the browser copies its opener's sessionStorage.
  let owner;
  try { if (performance.getEntriesByType("navigation")[0]?.type === "reload") owner = JSON.parse(sessionStorage.getItem("capy.workspace.owner")); } catch {}
  owner = typeof owner?.id === "string" && typeof owner?.epoch === "string" ? {id:owner.id,epoch:owner.epoch} : {id:crypto.randomUUID(),epoch:crypto.randomUUID()};
  try { sessionStorage.setItem("capy.workspace.owner",JSON.stringify(owner)); } catch {}
  store.holdOwner(owner.id);
  app.workspace_start(store.execute, JSON.stringify(owner));
  timer = setInterval(tick, 100); tick();
  document.addEventListener("visibilitychange", () => send({ type: document.hidden ? "suspend" : "resume" }));
  window.addEventListener("focus", () => send({type:"refresh_switcher"}));
  window.addEventListener("pagehide", () => send({ type: "suspend" }));
  window.addEventListener("pageshow", () => send({ type: "resume" }));
  window.addEventListener("beforeunload", e => { if (view?.dirty || view?.busy || view?.switcher_busy) { e.preventDefault(); e.returnValue = ""; } });
  return {
    ready,
    localize(){lastView=null;render();},
    wake,
    observe() { observePending = true; },
    send,
  };
}
