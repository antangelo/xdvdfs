export function isFilePickerAvailable() {
    return Boolean(window
        && window.showOpenFilePicker
        && window.showSaveFilePicker
        && window.showDirectoryPicker);
}

export function showOpenFilePicker(callback, _unused) {
    window.showOpenFilePicker().then((arr) => {
        const [file] = arr;
        callback(file);
    });
}

export function showSaveFilePicker(callback, suggestedName) {
    const opts = {
        types: [
        {
            description: 'Xbox ISO File',
            accept: { 'application/octet-stream': ['.iso', '.xiso'] }
        }
        ],
    };

    if (suggestedName) {
        opts.suggestedName = suggestedName;
    }

    window.showSaveFilePicker(opts).then(callback);
}

export function showDirectoryPicker(callback, _unused) {
    window.showDirectoryPicker().then(callback);
}
