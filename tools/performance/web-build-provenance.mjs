import assert from 'node:assert/strict';

export async function trackWasmBuild(call, evaluate, expected) {
  if (!expected) return { verify: async () => null, close: async () => {} };
  assert(/^[a-f0-9]{64}$/.test(expected), 'LAYER_WASM_SHA256 must be a SHA-256 hash');
  const { identifier } = await call('Page.addScriptToEvaluateOnNewDocument', { source: `(${() => {
    const original = window.fetch;
    window.fetch = async (...args) => {
      const response = await original.apply(window, args);
      if (new URL(response.url).pathname.endsWith('/pkg/layer_web_bg.wasm')) {
        response.clone().arrayBuffer().then(bytes => crypto.subtle.digest('SHA-256', bytes))
          .then(hash => { window.measuredWasmHash = [...new Uint8Array(hash)].map(byte => byte.toString(16).padStart(2, '0')).join(''); })
          .catch(error => { window.measuredWasmError = String(error); });
      }
      return response;
    };
  }})()` });
  return {
    async verify() {
      const deadline = Date.now() + 30000;
      while (Date.now() < deadline) {
        const result = await evaluate('({ hash: window.measuredWasmHash, error: window.measuredWasmError })');
        assert(!result.error, result.error);
        if (result.hash) { assert.equal(result.hash, expected, 'Loaded Wasm differs from the frozen build'); return result.hash; }
        await new Promise(resolve => setTimeout(resolve, 50));
      }
      throw Error('Loaded Wasm hash was not recorded');
    },
    close: () => call('Page.removeScriptToEvaluateOnNewDocument', { identifier }),
  };
}
