// SPDX-License-Identifier: MIT

const host = window.frameElement?.earthcraftWorkspace;
if (host) {
  async function select(method, args) {
    try {
      return await host[method](...args);
    } catch (error) {
      host.status(error.message);
      return null;
    }
  }
  globalThis.playgroundFiles = {
    ...host,
    open: (...args) => select('open', args),
    saveAs: (...args) => select('saveAs', args),
    persist: async (...args) => host.persist(...args),
    fetchImagery: async (...args) => host.fetchImagery(...args),
  };
}
try {
  const bindings = await import('./earthcraft.js');
  await bindings.default();
} catch (error) {
  const message = 'EarthCraft could not start. ' + (error.message || String(error));
  document.getElementById('earthcraft_loading')?.replaceChildren(message);
  host?.fail(message);
}
