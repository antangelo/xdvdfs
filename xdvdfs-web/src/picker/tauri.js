const { open, save } = window.__TAURI__.dialog;

export function showOpenFilePicker(callback) {
    open({
        multiple: false,
    }).then((maybeFile) => {
        if (maybeFile) callback(maybeFile);
    });
}

export function showSaveFilePicker(callback, suggestedName) {
    const opts = {
        filters: [{
            name: "Xbox ISO File",
            extensions: [".iso", ".xiso"],
        }],
    };

    if (suggestedName) {
        opts.defaultPath = suggestedName;
    }

    save(opts).then((maybeFile) => {
        if (maybeFile) callback(maybeFile);
    });
}

export function showDirectoryPicker(callback) {
    open({
        directory: true,
    }).then((maybeDir) => {
        if (maybeDir) callback(maybeDir);
    });
}
