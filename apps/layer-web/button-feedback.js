export function observeButtonFeedback(root) {
  let contact;
  const clear = e => {
    if (!contact || e?.pointerId != null && e.pointerId !== contact.id) return;
    contact.node.removeAttribute('data-pressed'); contact = null; observer.disconnect();
  };
  const valid = () => contact && contact.node.isConnected && contact.node.parentNode === contact.parent
    && !contact.node.matches(':disabled,[aria-disabled="true"],[hidden]')
    && !contact.node.closest('[inert],[hidden],.header-editing');
  const observer = new root.MutationObserver(() => { if (!valid()) clear(); });
  root.addEventListener('pointerdown', e => {
    clear();
    const node = e.target.closest?.('button,summary');
    if (e.button !== 0 || !e.isPrimary || !node) return;
    contact = {id:e.pointerId, node, parent:node.parentNode};
    if (!valid()) { clear(); return; }
    contact.bounds = node.getBoundingClientRect();
    node.setAttribute('data-pressed','');
    observer.observe(root.document.documentElement, {subtree:true, childList:true, attributes:true, attributeFilter:['disabled','aria-disabled','hidden','inert','class']});
  }, {capture:true, passive:true});
  root.addEventListener('pointermove', e => {
    if (contact?.id !== e.pointerId) return;
    if (!valid()) { clear(); return; }
    const r = contact.bounds;
    contact.node.toggleAttribute('data-pressed', e.clientX >= r.left && e.clientX < r.right && e.clientY >= r.top && e.clientY < r.bottom);
  }, {capture:true, passive:true});
  for (const type of ['pointerup','pointercancel','lostpointercapture']) root.addEventListener(type, clear, {capture:true, passive:true});
  for (const type of ['blur','resize','scroll','dragstart']) root.addEventListener(type, () => clear(), {capture:true, passive:true});
  root.addEventListener('keydown', e => { if (e.key === 'Escape') clear(); }, {capture:true});
}
