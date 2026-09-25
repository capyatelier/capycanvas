const surfaces = [
  ".dock-group:not(.expanded-panel) .panel-preview",
  ".collapsed-column",
  ".content-drawer",
  "#header .header-bar",
  "#header :is(.header-item, .header-overflow):not(.in-bar) :is(.header-tool, .header-menu > summary, #document-title, #system-clock, .system-status-tile)",
  "#header :is(.header-menu-labels, .header-status-placeholder, .drawing-selector, .drawing-tabs, .workspace-switcher)",
  "#zen-capy",
  "#canvas-status .proof-status",
  "#view-info",
].join(",");
const zen = ".zen-hidden > :is(.dock-group:not(.floating-panel), .collapsed-column, .chrome:not(#header)), .zen-hidden #header > *";
const corners = ["borderTopLeftRadius", "borderTopRightRadius", "borderBottomRightRadius", "borderBottomLeftRadius"];

export function createGlass({ app, canvas, workspace, connections, enabled, wake }) {
  const squircle = CSS.supports("corner-shape", "squircle");
  let dirty = true;
  function flush() {
    if (!dirty) return;
    dirty = false;
    const boxes = [];
    if (enabled()) {
      const origin = canvas.getBoundingClientRect();
      for (const node of workspace.querySelectorAll(surfaces)) {
        if (!node.checkVisibility({ visibilityProperty: true }) || node.closest(zen)) continue;
        const r = node.getBoundingClientRect(), style = getComputedStyle(node);
        if (r.width > 0 && r.height > 0)
          boxes.push(r.x - origin.x, r.y - origin.y, r.width, r.height, ...corners.map(corner => parseFloat(style[corner]) || 0));
      }
    }
    app.set_glass(new Float32Array(boxes), enabled() ? connections() : [], squircle);
  }
  function queue() {
    dirty = true;
    if (enabled()) wake();
  }
  workspace.addEventListener("transitionend", queue);
  return { flush, queue };
}
