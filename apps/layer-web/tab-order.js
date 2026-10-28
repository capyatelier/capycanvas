export function orderTabChildren(strip,tabs){
  if(tabs.every((tab,index)=>strip.children[index]===tab))return false;
  const pinned=!strip.moveBefore&&tabs.find(tab=>tab.contains(document.activeElement));
  if(pinned){
    const index=tabs.indexOf(pinned);
    for(const tab of tabs.slice(0,index))strip.insertBefore(tab,pinned);
    for(const tab of tabs.slice(index+1))strip.append(tab);
  }
  tabs.forEach((tab,index)=>{
    if(strip.children[index]===tab)return;
    if(strip.moveBefore&&tab.parentElement===strip)strip.moveBefore(tab,strip.children[index]??null);
    else strip.insertBefore(tab,strip.children[index]??null);
  });
  return true;
}
