// Original help UI. Browser settings URLs must be copied into the address bar:
// browsers deliberately prevent web pages from opening their internal settings.
export function gpuProblem({ secure, api, ui = true, stage }) {
  const title = "Could not initialize canvas";
  if (!secure) return [title, "Your browser needs a secure connection to access the GPU."];
  if (!ui) return [title, "Your browser is out of date."];
  if (!api) return [title, "WebGPU is not available in this browser."];
  if (stage === "adapter") return [title, "Your browser could not find a GPU adapter."];
  if (stage === "device") return [title, "Your browser found a GPU but could not start it."];
  return [title, "Your GPU could not initialize the canvas renderer."];
}

export function browserSupport() {
  return {
    secure: isSecureContext,
    api: !!navigator.gpu,
    ui: typeof HTMLElement.prototype.showPopover === "function",
  };
}

// Platform/browser hints select help only. Never use them to gate GPU startup.
export function gpuEnvironment({ userAgent = "", platform = "", maxTouchPoints = 0, userAgentData } = {}) {
  const os = userAgentData?.platform || platform;
  const ios = /iPhone|iPad|iPod|CriOS|FxiOS|EdgiOS/.test(userAgent)
    || (/Mac/.test(os + userAgent) && maxTouchPoints > 1);
  const system = ios ? "ios"
    : /Android/.test(os + userAgent) ? "android"
    : /CrOS|Chrome OS/.test(os + userAgent) ? "chromeos"
    : /Win/.test(os + userAgent) ? "windows"
    : /Mac/.test(os + userAgent) ? "mac"
    : /Linux/.test(os + userAgent) ? "linux" : "other";
  const browser = ios ? "webkit"
    : /Edg\//.test(userAgent) ? "edge"
    : /Firefox\//.test(userAgent) ? "firefox"
    : /Chrome\/|Chromium\//.test(userAgent) ? "chromium"
    : /Version\/.*Safari\//.test(userAgent) ? "safari" : "other";
  return { system, browser };
}

function element(tag, className, text) {
  const node = document.createElement(tag);
  if (className) node.className = className;
  if (text != null) node.textContent = text;
  return node;
}
function button(text, action, className = "") {
  const node = element("button", className, text);
  node.type = "button";
  node.addEventListener("click", action);
  return node;
}

export function showGpuNotice({ container, error, restart, restartLabel }) {
  container.removeAttribute("data-launch");
  const support = browserSupport();
  const [title, reason] = gpuProblem({ ...support, stage: error?.stage });
  const content = element("div", "gpu-help");
  const intro = element("p", "", "Capy Canvas is a GPU-accelerated drawing app and needs access to your GPU.");
  content.append(element("h1", "", title), element("p", "gpu-cause", reason));
  if (error !== undefined) content.append(element("p", "diagnostic-detail", error?.message ?? String(error)));
  if (restart) content.append(button(restartLabel, restart, "gpu-retry"));
  content.append(intro);
  const { system, browser } = gpuEnvironment(navigator);
  const address = (parent, url) => {
    const row = element("div", "gpu-address");
    const copy = button("Copy", async () => {
      try { await navigator.clipboard.writeText(url); copy.textContent = "Copied"; }
      catch { copy.textContent = "Copy manually"; }
    });
    copy.setAttribute("aria-label", `Copy ${url}`);
    row.append(element("code", "", url), copy);
    parent.append(row);
  };
  if (!isSecureContext) {
    content.append(element("h2", "", "Open a secure link"),
      element("p", "", "Use an https:// address, or localhost if you’re running the app yourself."));
  } else if (system === "ios") {
    content.append(element("h2", "", "Update your device"),
      element("p", "", "Update iOS or iPadOS to 26 or later, then reopen this page in Safari."));
  } else if (system === "android") {
    content.append(element("h2", "", "Try the latest Chrome"),
      element("p", "", "Update Chrome and Android, then reopen this page in Chrome. WebGPU needs Android 12 or later and a supported GPU. Some devices are not supported."));
  } else if (support.ui && ["chromium", "edge"].includes(browser)) {
    const scheme = browser === "edge" ? "edge" : "chrome";
    const name = browser === "edge" ? "Edge" : "Chrome";
    const steps = element("ol", "gpu-steps");
    const open = element("li", "", "Open your browser’s system settings:");
    address(open, `${scheme}://settings/system`);
    steps.append(open);
    for (const text of [
      "Turn on “Use graphics acceleration when available”, if available.",
      "Restart the browser.",
      "Reload this page.",
    ]) steps.append(element("li", "", text));
    content.append(steps);
    if (system === "linux") {
      content.append(element("p", "", "On Linux, enable “Override software rendering list”:"));
      address(content, `${scheme}://flags/#ignore-gpu-blocklist`);
      content.append(element("p", "", "If needed, enable “Unsafe WebGPU”:"));
      address(content, `${scheme}://flags/#enable-unsafe-webgpu`);
      const alternative = element("p", "", `Alternatively, force ${name} to use the Vulkan driver:`);
      const launch = element("span", "gpu-launch", `Relaunch ${name} with the `);
      launch.append(element("code", "", "--use-angle=vulkan"), " command line option.");
      alternative.append(launch);
      content.append(alternative);
    }
    const report = system === "linux" ? "Display Type should show ANGLE_VULKAN in the GPU debug page:"
      : `WebGPU should show “Hardware accelerated” in ${name}’s graphics report:`;
    content.append(element("p", "", report));
    address(content, `${scheme}://gpu`);
  } else {
    intro.append(" At the moment, the only supported browsers are:");
    const list = element("ul", "gpu-browsers");
    // Upstream WebGPU availability, not a browser allowlist. See the dated
    // sources and real-device testing limits in docs/development/web-packaging.md.
    for (const [platform, browsers] of [
      ["iPadOS", "Safari (iPadOS 26+)"],
      ["Android", "Chrome (Android 12+)"],
      ["Windows", "Chrome, Edge, Firefox 141+"],
      ["macOS", "Chrome, Edge, Safari 26+, Firefox 147+ (Apple Silicon)"],
      ["Linux (Wayland)", "Chrome, Edge"],
    ]) {
      const item = element("li");
      item.append(element("strong", "", platform), `: ${browsers}`);
      list.append(item);
    }
    content.append(list);
    if (!support.ui) content.append(element("h2", "", "Update your browser"),
      element("p", "", "Install the latest version of your browser, then reload this page."));
  }
  container.replaceChildren(content);
  container.hidden = false;
}

export function createLaunchCover({ container, asset, now = () => performance.now(), setTimer = setTimeout }) {
  const content = container.querySelector('.launch-splash');
  const mark = content.querySelector('.launch-capy');
  const caption = content.querySelector('.launch-caption');
  const blockedControls = new Set([...container.parentElement.children].filter(node=>node!==container));
  const controls = blocked => {
    if(blocked)for(const node of container.parentElement.children)
      if(node!==container&&!node.inert)blockedControls.add(node);
    for(const node of blockedControls)if(node.inert!==blocked)node.inert=blocked;
    if(!blocked)blockedControls.clear();
  };
  controls(true);
  container.addEventListener('transitionend', event => {
    if (event.target === container && event.propertyName === 'opacity' && container.dataset.launch === 'leaving')
      container.hidden = true;
  });
  const alignCaption = () => {
    const document = caption.ownerDocument;
    if (!document) return;
    const style = document.defaultView.getComputedStyle(caption);
    const context = document.createElement('canvas').getContext('2d');
    context.font = `${style.fontWeight} ${style.fontSize} ${style.fontFamily}`;
    const ink = context.measureText(caption.textContent);
    const baseline = (parseFloat(style.lineHeight) - ink.fontBoundingBoxAscent - ink.fontBoundingBoxDescent) / 2 + ink.fontBoundingBoxAscent;
    caption.style.transform = `translateY(${ink.actualBoundingBoxAscent - baseline}px)`;
  };
  caption.ownerDocument?.fonts.ready.then(alignCaption);
  let captionTimer;
  return {
    update(view, copy) {
      if (!view.visible && container.dataset.launch === undefined) return;
      container.style.setProperty('--launch-reveal', `${Number(copy.launch_reveal_ms)}ms`);
      container.style.setProperty('--launch-mark-size', `${copy.launch_mark_size}px`);
      container.style.setProperty('--launch-mark-fit', `calc(${copy.launch_mark_max_fraction} * min(100vw, 100dvh))`);
      container.style.setProperty('--launch-caption-gap', `${copy.launch_caption_gap}px`);
      container.style.setProperty('--launch-caption-size', `${copy.launch_caption_size}px`);
      container.style.setProperty('--launch-caption-color', `var(--${copy.launch_caption_color_role.replaceAll('_', '-')})`);
      if (copy.launch_mark && mark.dataset.icon === undefined) {
        mark.style.maskImage = `url(${JSON.stringify(asset(`icons/layer-${copy.launch_mark.icon}-symbolic.svg`))})`;
        mark.dataset.icon = copy.launch_mark.icon;
        container.style.setProperty('--launch-mark-ink-bottom', copy.launch_mark.ink_bottom);
      }
      captionTimer ??= setTimer(() => { caption.dataset.visible = ''; }, Math.max(0, Number(copy.launch_caption_delay_ms) - now()));
      controls(view.visible);
      if (view.visible) {
        const changed = caption.textContent !== copy.launch_caption;
        const detached = container.firstElementChild !== content;
        if (changed) caption.textContent = copy.launch_caption;
        if (detached) container.replaceChildren(content);
        if (changed || detached) alignCaption();
        container.hidden = false;
        container.dataset.launch = 'loading';
      } else if (!view.finished) {
        container.hidden = true;
      } else if (container.dataset.launch === 'loading') {
        container.dataset.launch = 'leaving';
      }
    },
    diagnostic() {
      controls(false);
      container.removeAttribute('data-launch');
      container.hidden = false;
    },
    get pending() { return container.dataset.launch === 'loading'; },
    get blocking() { return !container.hidden && container.dataset.launch === 'loading'; },
  };
}
