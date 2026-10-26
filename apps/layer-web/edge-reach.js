const surfaces = ".dock-group, .collapsed-column";
const sidewaysOwners = ".layer-swipe, .tool-choice-button, .number-slider, .dock-tabs, .drawer-tabs";
const swipeInputs = "input:not([type=checkbox]):not([type=radio]), textarea, select, [contenteditable=true], .layer-row[data-layer]";

export function edgeContactClass(target, canvas) {
  if (target === canvas) return "free";
  const surface = target.closest?.(surfaces);
  if (!surface) return null;
  if (target.closest(swipeInputs)) return "input";
  for (let node = target; node && node !== surface; node = node.parentElement) {
    if (node.matches(sidewaysOwners) || node.dataset.dragPickup === "immediate") return "pickup";
    if (node.dataset.dragPickup === "hold") return JSON.parse(node.dataset.workspaceDrag).item?.kind === "tile" ? "input" : "free";
    const style = getComputedStyle(node);
    if (style.touchAction === "pan-x") return "pickup";
    if (style.touchAction === "none") return node.matches(".tile-panel") ? "free" : "input";
    if (node.scrollWidth > node.clientWidth + 1 && /auto|scroll/.test(style.overflowX)) return "pickup";
  }
  return "free";
}

export function edgeResetClass(target, canvas) {
  const contact = edgeContactClass(target, canvas);
  return contact === "free" && target !== canvas && target.closest("button") ? "pickup" : contact;
}
