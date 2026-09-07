// Original help UI. Browser settings URLs must be copied into the address bar:
// browsers deliberately prevent web pages from opening their internal settings.
export function gpuProblem({ secure, api }) {
  if (!secure) return ["Open a secure link", "Use an address starting with https://, or localhost if you’re running the app locally."];
  if (!api) return ["This browser can’t draw here", "Update your browser, or try opening Capy Canvas in Chrome or Edge."];
  return ["Drawing isn’t available", "Your browser couldn’t start the canvas."];
}

export function showGpuNotice({ container, error, retry, element, button }) {
  const [title, reason] = gpuProblem({ secure: isSecureContext, api: !!navigator.gpu });
  const content = element("div", "gpu-help");
  content.append(element("h1", "", title), element("p", "", reason));
  const chromium = /Chrome\/|Chromium\/|Edg\//.test(navigator.userAgent);
  const browser = /Edg\//.test(navigator.userAgent) ? "Edge" : "Chrome";
  const scheme = browser.toLowerCase();
  if (isSecureContext && chromium) {
    const steps = element("ol");
    for (const text of [
      `Open ${browser} Settings → System.`,
      "Turn on “Use graphics acceleration when available”.",
      `Restart ${browser}, then come back here.`,
    ]) steps.append(element("li", "", text));
    content.append(steps);
  }
  content.append(button("Try again", retry, "gpu-retry"));
  const details = (parent, label) => {
    const node = element("details");
    node.append(element("summary", "", label));
    parent.append(node);
    return node;
  };
  const address = (parent, url, instruction) => {
    const row = element("div", "gpu-address");
    const copy = button("Copy", async () => {
      try { await navigator.clipboard.writeText(url); copy.textContent = "Copied"; }
      catch { copy.textContent = "Copy manually"; }
    });
    row.append(element("code", "", url), copy);
    parent.append(element("p", "", instruction), row);
  };
  const help = details(content, "More help");
  help.append(element("p", "", "Still not working? Update your browser or try another device."));
  address(help, `${scheme}://settings/system`, `For ${browser} settings, paste this into a new tab:`);
  address(help, `${scheme}://gpu`, "For a graphics report:");
  const advanced = details(help, "Experimental options");
  address(advanced, `${scheme}://flags/#enable-unsafe-webgpu`, "Unsafe WebGPU may enable experimental support, but can cause crashes. Restore Default if it causes problems.");
  const guide = element("a", "", "Troubleshooting guide");
  guide.href = "https://developer.chrome.com/docs/web-platform/webgpu/troubleshooting-tips";
  guide.target = "_blank";
  guide.rel = "noopener noreferrer";
  help.append(guide);
  const technical = details(help, "Technical details");
  technical.append(element("pre", "", String(error)));
  container.replaceChildren(content);
}
