export const browserDragThreshold=8;
// Chromium pans lists with touch but treats pen drags as mouse movement.
// Preserve native controls and drag surfaces; take only unclaimed vertical pans.
export function installPenScrolling() {
  let contact, suppressClick = false;
  document.addEventListener("pointerdown", e => {
    contact = null; suppressClick = false;
    const bubble=e.target.closest('[data-bubble-source]');
    if ((e.pointerType !== "pen" && !(e.pointerType === "touch" && (e.scrollbarContact||bubble))) || e.button || !e.isPrimary ||
        e.target.closest("input,textarea,select,[contenteditable=true]")) return;
    for (let node = e.target; node && node !== document.body; node = node.parentElement) {
      const style = getComputedStyle(node);
      if ((style.touchAction === "none" && node!==bubble) || style.touchAction === "pan-x") return;
      if (node.scrollHeight > node.clientHeight + 1 && /auto|scroll/.test(style.overflowY)) {
        contact = { id: e.pointerId, kind: e.pointerType, target: e.target, scrollbar: e.scrollbarContact, node, x: e.clientX, y: e.clientY, top: node.scrollTop, panning: false };
        break;
      }
    }
  });
  document.addEventListener("pointermove", e => {
    const current = contact;
    if (!current || current.id !== e.pointerId) return;
    const dx = e.clientX - current.x, dy = e.clientY - current.y;
    if (!current.panning) {
      if (Math.hypot(dx, dy) <= browserDragThreshold) return;
      if (e.defaultPrevented || Math.abs(dx) >= Math.abs(dy)) { contact = null; return; }
      // Retire pending row/menu holds just as native touch scrolling does.
      contact = null;
      const cancelled=new PointerEvent("pointercancel", { bubbles: true, pointerId: e.pointerId, pointerType: current.kind });
      cancelled.bubbleScroll=true;current.target.dispatchEvent(cancelled);
      current.panning = true; contact = current; suppressClick = true;
      current.node.setPointerCapture(e.pointerId);
    }
    e.preventDefault(); current.node.scrollTop = current.top - dy;
  }, { passive: false });
  const finish = e => {
    if (contact?.id !== e.pointerId) return;
    if (e.type === "lostpointercapture" && contact.panning && e.target !== contact.node) return;
    const current = contact; contact = null;
    if (current.panning && current.node.hasPointerCapture(e.pointerId)) current.node.releasePointerCapture(e.pointerId);
  };
  for (const event of ["pointerup", "pointercancel", "lostpointercapture"]) document.addEventListener(event, finish);
  document.addEventListener("gotpointercapture", e => {
    if (contact?.id === e.pointerId && !contact.panning && e.target !== contact.target && e.target !== contact.scrollbar) contact = null;
  });
  document.addEventListener("workspace-context-claimed", () => { contact = null; });
  window.addEventListener("blur", () => { contact = null; });
  document.addEventListener("click", e => {
    if (suppressClick) { suppressClick = false; e.preventDefault(); e.stopImmediatePropagation(); }
  }, true);
}
