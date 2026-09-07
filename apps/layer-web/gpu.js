// Original help UI. Browser settings URLs must be copied into the address bar:
// browsers deliberately prevent web pages from opening their internal settings.
export function gpuProblem({ secure, api, error }) {
  if (!secure) return ["A secure connection is needed", "Open Capy Canvas over HTTPS, or use localhost when running it on your own computer."];
  if (!api) return ["WebGPU is not available", "This browser is not exposing the graphics API that Capy Canvas needs for drawing."];
  if (/adapter|no gpu/i.test(String(error))) return ["No compatible GPU was found", "WebGPU is available, but the browser could not connect to a supported graphics adapter."];
  return ["The GPU canvas could not start", "The browser could not initialize the drawing surface. Restarting the browser or updating your graphics driver may help."];
}

export function showGpuNotice({ container, error, retry, element, button }) {
  const [title, reason] = gpuProblem({ secure: isSecureContext, api: !!navigator.gpu, error });
  const content = element("div", "gpu-help");
  content.append(element("h1", "", title), element("p", "", reason),
    element("p", "", "You can still explore the tools, arrange panels, and change preferences. Drawing requires a working GPU; there is no software canvas fallback."));
  const steps = element("ol");
  for (const text of [
    "Update your browser and operating system, then restart the browser.",
    "Enable graphics acceleration in your browser settings if that option is available. On desktop computers, also check for graphics driver updates.",
    "If WebGPU remains unavailable, try another up-to-date browser or a device with supported graphics hardware. Support varies by browser, operating system, and GPU.",
  ]) steps.append(element("li", "", text));
  content.append(steps);
  const details = (label) => {
    const node = element("details");
    node.append(element("summary", "", label));
    content.append(node);
    return node;
  };
  const address = (parent, url, instruction) => {
    const row = element("div", "gpu-address");
    const copy = button("Copy", async () => {
      try { await navigator.clipboard.writeText(url); copy.textContent = "Copied"; }
      catch { copy.textContent = "Select and copy the address"; }
    });
    row.append(element("code", "", url), copy);
    parent.append(element("p", "", instruction), row);
  };
  const chrome = details("Chrome / Edge settings");
  chrome.open = /Chrome\/|Chromium\/|Edg\//.test(navigator.userAgent);
  const scheme = /Edg\//.test(navigator.userAgent) ? "edge" : "chrome";
  address(chrome, `${scheme}://settings/system`, "Paste this address into a new tab. Turn on “Use graphics acceleration when available”, then restart the browser.");
  address(chrome, `${scheme}://gpu`, "Check the WebGPU entry and “Problems Detected” for disabled acceleration, a blocked driver, or an unsupported device.");
  const advanced = element("details");
  advanced.append(element("summary", "", "Experimental WebGPU support"));
  address(advanced, `${scheme}://flags/#enable-unsafe-webgpu`, "On platforms with experimental support, this flag may make WebGPU available after a browser restart. It can bypass driver safeguards and cause instability; it cannot add missing hardware support. Restore Default if it causes problems.");
  advanced.append(element("p", "", "On Linux, check your Mesa or vendor graphics drivers. Avoid forcing Vulkan compositor flags: some Wayland configurations cannot start with them."));
  chrome.append(advanced);
  const guide = element("a", "", "Browser GPU troubleshooting guide");
  guide.href = "https://developer.chrome.com/docs/web-platform/webgpu/troubleshooting-tips";
  guide.target = "_blank";
  guide.rel = "noopener noreferrer";
  chrome.append(guide);
  const technical = details("Technical details");
  technical.append(element("pre", "", String(error)));
  content.append(button("Try GPU again", retry, "gpu-retry"));
  container.replaceChildren(content);
}
