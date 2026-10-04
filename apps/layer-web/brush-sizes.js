export function brushSizeGrid({ app, catalog, state, dispatch, element, button }) {
  const view = catalog.brush_size_grid, grid = element("div", "size-grid");
  grid.style.setProperty("--size-tile", `${view.tile_size}px`);
  grid.style.setProperty("--size-columns", view.max_columns);
  grid.style.setProperty("--size-gap", `${view.gap}px`);
  grid.style.setProperty("--size-fade", `${view.fade_height}px`);
  const buttons = view.presets.map(({ value, label, preview_diameter }) => {
    const choice = button("", () => dispatch({ type: "set_brush_size", value }), "size-button");
    choice.title = `${label} px`;
    choice.setAttribute("aria-label", choice.title);
    choice.onpointerenter = () => { choice.title = app.action_tooltip(`${label} px`, { type: "set_brush_size", value }); };
    choice.dataset.size = label;
    const dot = element("span", "size-dot");
    dot.style.width = dot.style.height = `${preview_diameter}px`;
    choice.append(dot, element("span", "size-label", label));
    grid.append(choice);
    return [value, choice];
  });
  grid.refresh = () => {
    const size = state().brush.diameter;
    for (const [value, choice] of buttons) {
      const pressed = String(value === size);
      if (choice.getAttribute("aria-pressed") !== pressed) choice.setAttribute("aria-pressed", pressed);
    }
  };
  grid.refresh();
  return grid;
}
