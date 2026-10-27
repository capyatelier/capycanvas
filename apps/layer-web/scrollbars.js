const dirtyScrollbars = new Set();
let scrollbarFrame;
function queueScrollbar(read) {
  dirtyScrollbars.add(read);
  if (scrollbarFrame) return;
  scrollbarFrame = requestAnimationFrame(() => {
    scrollbarFrame = null;
    const writes = [...dirtyScrollbars].map(read => read());
    dirtyScrollbars.clear();
    for (const write of writes) write();
  });
}
export function addScrollbar(panel, frame, content = panel) {
  const thumb = document.createElement("div"); thumb.className = "scroll-thumb";
  thumb.setAttribute("aria-hidden", "true");
  frame.append(thumb);
  let origin;
  const read = () => {
    const visible = panel.clientHeight, total = panel.scrollHeight, top = panel.scrollTop;
    const overflow = total - visible;
    const height = total ? Math.max(28, visible ** 2 / total) : 0;
    return () => {
      thumb.hidden = !visible || overflow <= 1;
      thumb.style.height = `${height}px`;
      thumb.style.top = `${overflow > 0 ? (top / overflow) * (visible - height) : 0}px`;
    };
  };
  const update = () => queueScrollbar(read);
  panel.addEventListener("scroll", update);
  const resize = new ResizeObserver(update);
  resize.observe(panel);
  if (content !== panel) resize.observe(content);
  new MutationObserver(update).observe(panel, {
    childList: true,
    subtree: true,
  });
  thumb.addEventListener("pointerdown", (e) => {
    if (e.pointerType !== "mouse") {
      e.stopPropagation();
      const target = document.elementFromPoint(e.clientX, e.clientY);
      if (target && target !== thumb) {
        const contact = new PointerEvent("pointerdown", { bubbles: true, pointerId: e.pointerId, pointerType: e.pointerType, isPrimary: e.isPrimary, button: e.button, buttons: e.buttons, clientX: e.clientX, clientY: e.clientY });
        contact.scrollbarContact = thumb;
        target.dispatchEvent(contact);
      }
      return;
    }
    origin = [e.clientY, panel.scrollTop];
    thumb.setPointerCapture(e.pointerId);
    e.preventDefault();
  });
  thumb.addEventListener("pointermove", (e) => {
    if (e.pointerType === "mouse" && origin && thumb.hasPointerCapture(e.pointerId))
      panel.scrollTop =
        origin[1] +
        ((e.clientY - origin[0]) * (panel.scrollHeight - panel.clientHeight)) /
          (panel.clientHeight - thumb.offsetHeight);
  });
  return thumb;
}
