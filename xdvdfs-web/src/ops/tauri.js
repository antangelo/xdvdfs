const invoke = window.__TAURI__.invoke;
const { open } = window.__TAURI__.shell;
const { appWindow } = window.__TAURI__.window;

export async function pack_image(sourcePath, destPath, progessCallback) {
    const unlisten = await appWindow.listen('progress_callback', (event) => {
        progessCallback(event.payload);
    });

    try {
        return await invoke('pack_image', { sourcePath, destPath });
    }
    finally {
        unlisten();
    }
}

export async function unpack_image(sourcePath, destPath, progessCallback) {
    const unlisten = await appWindow.listen('progress_callback', (event) => {
        progessCallback(event.payload);
    });

    try {
        return await invoke('unpack_image', { sourcePath, destPath });
    }
    finally {
        unlisten();
    }
}

export async function compress_image(sourcePath, destPath, progressCallback, compressCallback) {
    const unlistenPC = await appWindow.listen('progress_callback', (event) => {
        progressCallback(event.payload);
    });

    const unlistenCC = await appWindow.listen('compress_callback', (event) => {
        compressCallback(event.payload);
    });

    try {
        return await invoke('compress_image', { sourcePath, destPath });
    }
    finally {
        unlistenPC();
        unlistenCC();
    }
}

export function open_url(url) {
    open(url);
}
