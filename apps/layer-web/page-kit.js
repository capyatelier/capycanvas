import {bindCopy} from './localization.js';
import {addScrollbar} from './scrollbars.js';

export function createPageKit({element,icon}) {
  function group(title,description) {
    const section=element('section','settings-group');
    if(description){
      const heading=element('div','settings-heading'),labels=element('div');
      if(title)labels.append(element('h3','',title));
      labels.append(element('p','settings-description',description));
      heading.append(labels);section.append(heading);
    }else if(title)section.append(element('h3','',title));
    const list=element('div','preference-group');section.append(list);
    return {section,list};
  }
  function row({title,description,suffix,line=element('div','preference-row')}) {
    line.classList.add('preference-row');const text=element('div','preference-text'),label=element('label','',title);
    text.append(label);if(description)text.append(element('p','',description));
    line.append(text);if(suffix)line.append(suffix);
    return {line,text,label};
  }
  function entry({label,placeholder,value='',id}) {
    const input=element('input','preference-entry preference-input');input.type='text';input.value=value;
    if(id)input.id=id;
    if(typeof label==='function')bindCopy(input,label,'ariaLabel');else input.setAttribute('aria-label',label);
    if(placeholder!=null){if(typeof placeholder==='function')bindCopy(input,placeholder,'placeholder');else input.placeholder=placeholder;}
    return input;
  }
  function entryRow({title,value,id,input=entry({label:title,value,id})}) {
    const result=row({title});input.classList.add('preference-entry');input.classList.add('preference-input');
    if(id)input.id=id;
    result.line.classList.add('preference-entry-row');
    result.text.append(input);result.label.htmlFor=id;
    const edit=icon('pencil');edit.setAttribute('aria-hidden','true');result.line.append(edit);
    result.line.addEventListener('click',()=>input.focus());
    return {...result,input};
  }
  function comboRow({title,control,subtitle=true}) {
    control.classList.add('preference-row','preference-combo-row');
    control.classList.toggle('preference-combo-inline',!subtitle);
    control.prepend(element('span','preference-combo-title',title));return control;
  }
  function body(...children) {
    const root=element('div','preference-body'),viewport=element('div','preference-viewport'),content=element('div','preference-content');content.append(...children);viewport.append(content);root.append(viewport);
    const scrollbar=addScrollbar(viewport,root,content);
    return {root,content,dispose:()=>scrollbar.disposeScrollbar()};
  }
  function search(input) {
    const root=element('div','preference-search'),glyph=icon('search');glyph.setAttribute('aria-hidden','true');
    root.append(glyph,input);return root;
  }
  const fields=(...children)=>{const root=element('div','preference-fields');root.append(...children);return root;};
  const numberRow=control=>{control.classList.add('preference-number');control.setShowUnits(false);return control;};
  function footer({start=[],actions=[],separator=false}={}) {
    const root=element('footer','preference-footer');root.classList.toggle('separated',separator);
    if(start.length){const leading=element('div','preference-footer-start');leading.append(...start);root.append(leading);}
    root.append(...actions);return root;
  }
  return {group,row,entry,entryRow,comboRow,body,search,fields,numberRow,footer};
}
