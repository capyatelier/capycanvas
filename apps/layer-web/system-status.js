// Native/browser chrome state stays outside the drawing render loop.
export function createSystemStatus({element, changed}) {
  const root = element("div", "system-status");
  root.id = "system-status"; root.hidden = true;
  const clock = element("time"); clock.id = "system-clock";
  const battery = element("span", "system-battery"); battery.id = "system-battery";
  battery.setAttribute("role", "img"); battery.hidden = true;
  const charging = element("span", "battery-charging", "ϟ");
  const shell = element("span", "battery-shell");
  const fill = element("span", "battery-fill"), percent = element("span", "battery-percent");
  shell.append(fill, percent); battery.append(charging, shell); root.append(clock, battery);
  const displayMode = matchMedia("(display-mode: fullscreen)");
  let timer, manager, active = false;
  function updateClock() {
    const now = new Date();
    clock.textContent = new Intl.DateTimeFormat(navigator.languages, {hour:"numeric", minute:"2-digit"}).format(now);
    clock.dateTime = now.toISOString();
  }
  function tick() {
    clearTimeout(timer);
    if (!active || document.hidden) return;
    updateClock();
    timer = setTimeout(tick, 60000 - Date.now() % 60000 + 20);
  }
  function updateBattery() {
    if (!manager || !Number.isFinite(manager.level) || manager.level < 0 || manager.level > 1) {
      battery.hidden = true; return;
    }
    const level = Math.round(manager.level * 100), low = level <= 15 && !manager.charging;
    percent.textContent = new Intl.NumberFormat(navigator.languages).format(level);
    fill.style.width = `${level}%`;
    charging.hidden = !manager.charging;
    battery.classList.toggle("low", low);
    battery.setAttribute("aria-label", `Battery ${level}%${manager.charging ? ", charging" : low ? ", low" : ""}`);
    battery.title = battery.getAttribute("aria-label"); battery.hidden = false;
  }
  function sync() {
    // The media query also covers browser-owned fullscreen (e.g. Chrome F11),
    // which does not set document.fullscreenElement or emit fullscreenchange.
    active = !!document.fullscreenElement || displayMode.matches;
    root.hidden = !active;
    document.body.classList.toggle("window-fullscreen", active);
    tick(); changed(active);
  }
  document.addEventListener("fullscreenchange", sync);
  displayMode.addEventListener("change", sync);
  window.addEventListener("pageshow", sync);
  document.addEventListener("visibilitychange", tick);
  window.addEventListener("languagechange", () => {tick(); updateBattery();});
  // Optional and asynchronous: battery discovery never delays UI or canvas.
  if (navigator.getBattery) Promise.resolve().then(() => navigator.getBattery()).then(value => {
    manager = value;
    for (const event of ["levelchange", "chargingchange"]) manager.addEventListener(event, updateBattery);
    updateBattery();
  }).catch(() => { battery.hidden = true; });
  sync();
  return {root, sync, async setFullscreen(enabled) {
    if (enabled === (!!document.fullscreenElement || displayMode.matches)) return;
    if (enabled) await document.documentElement.requestFullscreen();
    else if (document.fullscreenElement) await document.exitFullscreen();
    else throw new Error("Use your browser’s fullscreen control to leave full screen.");
    sync();
  }};
}
