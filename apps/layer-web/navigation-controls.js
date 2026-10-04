export function createNavigationControls({ element, button, icon, dispatch, commands, prefix }) {
  const root = element("div", "navigator-buttons");
  const buttons = commands.map(id => {
    const node = button("", () => dispatch({ type: "invoke", command: id }));
    node.dataset.navigatorCommand = id;
    if (prefix) node.id = `${prefix}-${id}`;
    root.append(node);
    return [id, node];
  });
  let key = "";
  root.update = states => {
    const current = buttons.map(([id]) => states.find(c => c.id === id));
    const next = JSON.stringify(current);
    if (key === next) return;
    key = next;
    buttons.forEach(([, node], i) => {
      const command = current[i];
      if (!node.firstChild) node.append(icon(command.icon));
      node.title = command.tooltip;
      node.setAttribute("aria-label", command.label);
      node.setAttribute("aria-pressed", String(command.selected));
      node.disabled = !command.enabled;
    });
  };
  return root;
}
