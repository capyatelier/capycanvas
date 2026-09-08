// Original help UI. Browser settings URLs must be copied into the address bar:
// browsers deliberately prevent web pages from opening their internal settings.
export function gpuProblem({ secure, api, stage }) {
  const title = "Could not initialize canvas";
  if (!secure) return [title, "Your browser needs a secure connection to access the GPU."];
  if (!api) return [title, "WebGPU is not available in this browser."];
  if (stage === "adapter") return [title, "Your browser could not find a GPU adapter."];
  if (stage === "device") return [title, "Your browser found a GPU but could not start it."];
  return [title, "Your GPU could not initialize the canvas renderer."];
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

export function showGpuNotice({ container, error, element, button }) {
  const [title, reason] = gpuProblem({ secure: isSecureContext, api: !!navigator.gpu, stage: error?.stage });
  const content = element("div", "gpu-help");
  const intro = element("p", "", "Capy Canvas is a GPU-accelerated drawing app and needs access to your GPU.");
  content.append(element("h1", "", title), element("p", "gpu-cause", reason), intro);
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
  } else if (["chromium", "edge"].includes(browser)) {
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
    // sources and real-device testing limits in docs/web-packaging.md.
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
  }
  container.replaceChildren(content);
}
