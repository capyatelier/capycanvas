// Original help UI. Browser settings URLs must be copied into the address bar:
// browsers deliberately prevent web pages from opening their internal settings.
export function gpuProblem({ secure, api }) {
  const title = "Could not initialize canvas";
  if (!secure) return [title, "Your browser needs a secure connection to access the GPU."];
  if (!api) return [title, "Your browser does not have WebGPU enabled."];
  return [title, "Your browser could not find a GPU adapter."];
}

export function showGpuNotice({ container, error, retry, element, button }) {
  const [title, reason] = gpuProblem({ secure: isSecureContext, api: !!navigator.gpu });
  const content = element("div", "gpu-help");
  content.append(element("h1", "", title), element("p", "gpu-cause", reason),
    element("p", "gpu-intro", "Capy Canvas is a GPU-accelerated drawing app. It needs WebGPU to use your graphics hardware. Try the steps below to start drawing."));
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
    content.append(element("h2", "", "Try these steps in Chrome"),
      element("p", "gpu-hint", "Copy each address into a new tab."),
      element("p", "gpu-caution", "Steps 2–3 are experimental and may reduce browser protections or cause crashes. Restore Default if they cause problems."));
    const steps = element("ol", "gpu-steps");
    for (const [heading, text, url] of [
      ["Update Chrome", "", "chrome://settings/help"],
      ["Enable WebGPU", "Set “Unsafe WebGPU” to Enabled.", "chrome://flags/#enable-unsafe-webgpu"],
      ["Allow blocked graphics hardware if needed", "Try setting “Override software rendering list” to Enabled.", "chrome://flags/#ignore-gpu-blocklist"],
      ["Relaunch Chrome", "Use the Relaunch button on the flags page."],
      ["Return to Capy Canvas", "Click Try again below."],
    ]) {
      const step = element("li");
      step.append(element("h3", "", heading));
      if (text) step.append(element("p", "", text));
      if (url) address(step, url);
      steps.append(step);
    }
    content.append(steps);
  } else {
    content.append(element("h2", "", "Try an updated browser"),
      element("p", "", "Update your browser, or open Capy Canvas in Chrome. If drawing still won’t start, try another device."));
  }
  content.append(button("Try again", retry, "gpu-retry"));
  const technical = element("details");
  technical.append(element("summary", "", "Technical details"));
  if (isSecureContext && chromium) {
    technical.append(element("p", "", "Still not working? Update your graphics driver or try another device."));
    technical.append(element("p", "", "Check Chrome’s graphics report:"));
    address(technical, "chrome://gpu");
  }
  technical.append(element("pre", "", String(error)));
  content.append(technical);
  container.replaceChildren(content);
}
