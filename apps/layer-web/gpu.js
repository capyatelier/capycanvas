// Original help UI. Browser settings URLs must be copied into the address bar:
// browsers deliberately prevent web pages from opening their internal settings.
export function gpuProblem({ secure, api }) {
  const title = "Could not initialize canvas";
  if (!secure) return [title, "Your browser needs a secure connection to access the GPU."];
  if (!api) return [title, "WebGPU is not available in this browser."];
  return [title, "Your browser could not find a GPU adapter."];
}

export function showGpuNotice({ container, error, retry, element, button }) {
  const [title, reason] = gpuProblem({ secure: isSecureContext, api: !!navigator.gpu });
  const content = element("div", "gpu-help");
  content.append(element("h1", "", title), element("p", "gpu-cause", reason),
    element("p", "gpu-intro", "Capy Canvas is a GPU-accelerated drawing app and needs access to your GPU."));
  const chromium = /Chrome\/|Chromium\/|Edg\//.test(navigator.userAgent);
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
  } else if (chromium) {
    const steps = element("ol", "gpu-steps");
    const open = element("li", "", "Open your browser’s system settings:");
    address(open, "chrome://settings/system");
    steps.append(open);
    for (const text of [
      "Turn on “Use graphics acceleration when available”, if available.",
      "Restart the browser.",
      "Reload this page.",
    ]) steps.append(element("li", "", text));
    const linux = /Linux/.test(navigator.userAgent) && !/Android|CrOS/.test(navigator.userAgent);
    if (linux) {
      const extra = element("li", "", "On Linux, if it still fails, set “Override software rendering list” to Enabled.");
      address(extra, "chrome://flags/#ignore-gpu-blocklist");
      extra.append(element("p", "", "If that still doesn’t work, set “Unsafe WebGPU” to Enabled."));
      address(extra, "chrome://flags/#enable-unsafe-webgpu");
      steps.append(extra);
    }
    content.append(steps, element("p", "", "Check Chrome’s graphics report:"));
    address(content, "chrome://gpu");
    if (linux) content.append(element("p", "gpu-caution", "Experimental flags may cause crashes or reduce browser protections. Restore Default if needed."));
  } else {
    content.append(element("h2", "", "Try an updated browser"),
      element("p", "", "Update your browser or open Capy Canvas in Chrome, then try again."));
  }
  const actions = element("div", "gpu-actions");
  actions.append(button("Try again", retry, "gpu-retry"));
  const technical = element("details");
  technical.append(element("summary", "", "Technical details"));
  technical.append(element("pre", "", String(error)));
  actions.append(technical);
  content.append(actions);
  container.replaceChildren(content);
}
