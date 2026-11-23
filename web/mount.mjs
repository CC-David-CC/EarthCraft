// SPDX-License-Identifier: MIT

const validRevision = /^[a-f0-9]{64}$/;
const invalid = /[\\/:\u0000-\u001f]/;
const nasaTile = /^https:\/\/gibs\.earthdata\.nasa\.gov\/wmts\/epsg3857\/best\/BlueMarble_ShadedRelief_Bathymetry\/default\/GoogleMapsCompatible_Level8\/([0-8])\/(\d{1,3})\/(\d{1,3})\.jpeg$/;

function toPath(value, allowRoot = false) {
  if (typeof value !== 'string') throw Error('Choose a path in My files.');
  const path = value.replace(/^(?:browser:)?\//, '');
  if (path === '' && allowRoot) return path;
  if (path.split('/').some(part => part === '' || part === '.' || part === '..' || invalid.test(part))) {
    throw Error('Choose a path in My files.');
  }
  if (path === '.app' || path.startsWith('.app/')) throw Error('Choose a folder outside app settings.');
  return path;
}

function baseName(path) {
  return path.split('/').at(-1);
}

export function mount(host) {
  const owner = host.storage.workspaceOwner();
  const session = host.session();
  const abort = new AbortController();
  const {signal} = abort;
  const element = document.createElement('section');
  element.className = 'window-content earthcraft-workspace';
  const style = document.createElement('link');
  style.rel = 'stylesheet';
  style.href = new URL('./workspace.css', import.meta.url).href;
  const frame = document.createElement('iframe');
  frame.title = 'EarthCraft globe';
  const status = document.createElement('p');
  status.setAttribute('role', 'status');
  status.textContent = 'Starting EarthCraft…';
  element.append(style, frame, status);

  let context, opened, releaseWriter, bound;
  let unsaved = false;
  let transferring = false;
  let pending = Promise.resolve();
  let startReady, startFailed;
  const started = new Promise((resolve, reject) => {
    startReady = resolve;
    startFailed = reject;
  });
  started.catch(() => {});
  const timer = setTimeout(() => startFailed(Error('EarthCraft took too long to start.')), 90000);
  signal.addEventListener('abort', () => startFailed(signal.reason), {once: true});

  function workspace() {
    if (signal.aborted || owner !== host.storage.workspaceOwner() || session !== host.session() || !context?.valid()) {
      throw new DOMException('This Files workspace ended.', 'AbortError');
    }
    if (transferring) throw Error('Files are being transferred.');
    return context.files;
  }

  async function read(path) {
    const {sha256, data} = await workspace().read(path, {signal});
    workspace();
    if (!validRevision.test(sha256)) throw Error('Files returned an invalid revision.');
    return {path: 'browser:/' + path, name: baseName(path), bytes: data, sha256};
  }

  async function makeParents(path) {
    let parent = '';
    for (const part of path.split('/').slice(0, -1)) {
      const folder = parent ? parent + '/' + part : part;
      const exists = async () => {
        const {entries} = await workspace().list(parent, {signal});
        return entries.some(entry => !entry.deleted && entry.directory && entry.name === part);
      };
      if (!await exists()) {
        try {
          await workspace().mkdir(folder, {signal});
        } catch (error) {
          if (error?.status !== 409 || !await exists()) throw error;
        }
      }
      parent = folder;
    }
  }

  function persist(location, bytes, expected) {
    const write = pending.then(async () => {
      const path = toPath(location);
      if (expected !== null && !validRevision.test(expected)) throw Error('Files returned an invalid revision.');
      await makeParents(path);
      try {
        const result = await workspace().write(path, new Blob([bytes]), expected, {signal});
        workspace();
        if (!validRevision.test(result.sha256)) throw Error('Files did not confirm the write.');
        return result.sha256;
      } catch (error) {
        if (error?.status === 409) throw Error('This file already exists or changed. Save As under a new name to keep both copies.');
        throw error;
      }
    });
    pending = write.catch(() => {});
    return write;
  }

  frame.earthcraftWorkspace = {
    onOpen(callback) { opened = callback; },
    ready() { startReady(); },
    fail(message) { startFailed(Error(message)); },
    setUnsaved(dirty) { unsaved = dirty; },
    status(message) { if (!signal.aborted) status.textContent = message; },
    bind(location, sha256) {
      workspace();
      const path = location == null ? null : toPath(location);
      if (path && !validRevision.test(sha256)) throw Error('Files returned an invalid revision.');
      bound = path;
      host.desktop.updateAccountFileBinding(host.w, {owner, path, sha256: path ? sha256 : null});
    },
    async open({types}) {
      workspace();
      const selection = await host.storage.pickWorkspaceFile({
        extensions: types.map(type => '.' + type), signal,
      });
      return selection ? [await read(toPath(selection))] : null;
    },
    async saveAs({name, types}) {
      workspace();
      const folder = await host.storage.pickWorkspaceFile({foldersOnly: true, signal});
      if (folder == null) return null;
      const choice = await host.ui.prompt('File name', {title: 'Save EarthCraft file', value: name, confirmLabel: 'Save', signal});
      if (choice == null) return null;
      workspace();
      let file = choice.trim();
      if (file === '' || file === '.' || file === '..' || invalid.test(file)) throw Error('Enter a file name.');
      if (!types.some(type => file.toLowerCase().endsWith('.' + type))) file += '.' + types[0];
      const parent = toPath(folder, true);
      return {path: 'browser:/' + (parent ? parent + '/' + file : file)};
    },
    async fetchImagery(url, timeoutMs) {
      const tile = nasaTile.exec(url);
      if (!tile || Number(tile[2]) >= 2 ** Number(tile[1]) || Number(tile[3]) >= 2 ** Number(tile[1])) {
        throw Error('The requested image is outside the NASA Blue Marble tiles.');
      }
      const requestSignal = timeoutMs > 0 ? AbortSignal.any([signal, AbortSignal.timeout(timeoutMs)]) : signal;
      const response = await fetch(url, {
        signal: requestSignal, mode: 'cors', credentials: 'omit', redirect: 'error', referrerPolicy: 'no-referrer',
      });
      return {status: response.status, bytes: new Uint8Array(await response.arrayBuffer())};
    },
    persist,
    write(path, bytes) {
      persist(path, bytes, null).then(
        () => { status.textContent = 'Saved ' + baseName(path); },
        error => { if (!signal.aborted) status.textContent = error.message; },
      );
    },
  };

  const ready = (async () => {
    context = await host.storage.workspaceFileContext();
    if (context.owner !== owner) throw Error('EarthCraft belongs to another Files workspace.');
    workspace();
    frame.src = new URL('./native/index.html', import.meta.url).href;
    await started;
    workspace();
    status.textContent = 'EarthCraft is ready';
    releaseWriter = context.registerFileWriter?.(async () => {
      await pending;
      if (unsaved) throw Error('Save the EarthCraft places before transferring Files.');
      transferring = true;
      frame.inert = true;
      return () => {
        frame.inert = false;
        transferring = false;
      };
    });

    const launch = host.w.launchContext;
    if (launch) {
      if (launch.storage !== 'account' || launch.owner !== owner) throw Error('Open the places file from My files.');
      const file = await read(toPath(launch.path));
      if (!launch.restore && launch.sha256 && launch.sha256 !== file.sha256) throw Error('The places file changed. Open its current revision.');
      opened(file);
    }
  })().finally(() => clearTimeout(timer));
  ready.catch(error => { if (!signal.aborted) status.textContent = error.message; });

  return {
    element,
    ready,
    focus: () => frame.focus(),
    inspect: () => ({path: bound ?? null, dirty: unsaved}),
    async requestClose() {
      await pending;
      return !unsaved || host.ui.confirm('Discard unsaved changes to EarthCraft places?', {confirmLabel: 'Discard', signal});
    },
    dispose() {
      abort.abort();
      releaseWriter?.();
    },
  };
}
