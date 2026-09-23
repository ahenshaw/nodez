//! Where the app keeps its files.
//!
//! On the desktop that is the disk. A browser has no disk to offer, so there
//! the same paths are keys in the page's local storage: a graph saved there is
//! still there when the page is next opened, and a group kept in `groups/` is
//! read back like one in a directory. What the preview writes is for use
//! elsewhere, so in a browser it is downloaded instead.

pub use imp::*;

#[cfg(not(all(target_arch = "wasm32", feature = "app")))]
mod imp {
    use std::path::Path;

    pub fn read(path: &str) -> Result<String, String> {
        std::fs::read_to_string(path).map_err(|e| e.to_string())
    }

    /// Write a file, and say where it went.
    pub fn write(path: &str, text: &str) -> Result<String, String> {
        if let Some(dir) = Path::new(path).parent().filter(|dir| !dir.as_os_str().is_empty()) {
            std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
        std::fs::write(path, text).map_err(|e| e.to_string())?;
        Ok(path.to_owned())
    }

    /// Hand over a file made for use outside the app.
    pub fn export(path: &str, text: &str) -> Result<String, String> {
        write(path, text)
    }

    /// Every `.json` file in a directory, by name, sorted so the same
    /// directory reads the same way twice.
    pub fn read_json_dir(dir: &str) -> Vec<(String, Result<String, String>)> {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return Vec::new();
        };
        let mut paths: Vec<_> = entries
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|path| path.extension().is_some_and(|e| e == "json"))
            .collect();
        paths.sort();
        paths
            .into_iter()
            .map(|path| {
                let text = std::fs::read_to_string(&path).map_err(|e| e.to_string());
                (path.display().to_string(), text)
            })
            .collect()
    }
}

#[cfg(all(target_arch = "wasm32", feature = "app"))]
mod imp {
    use wasm_bindgen_futures::js_sys;
    use wasm_bindgen_futures::wasm_bindgen::{JsCast, JsValue};

    fn storage() -> Result<web_sys::Storage, String> {
        web_sys::window()
            .and_then(|window| window.local_storage().ok().flatten())
            .ok_or_else(|| "this browser has no local storage".to_owned())
    }

    fn js(e: JsValue) -> String {
        e.as_string().unwrap_or_else(|| format!("{e:?}"))
    }

    pub fn read(path: &str) -> Result<String, String> {
        storage()?
            .get_item(path)
            .map_err(js)?
            .ok_or_else(|| format!("nothing saved as {path} in this browser"))
    }

    pub fn write(path: &str, text: &str) -> Result<String, String> {
        storage()?.set_item(path, text).map_err(js)?;
        Ok(format!("{path} (browser storage)"))
    }

    /// Download the text as a file of that name.
    pub fn export(path: &str, text: &str) -> Result<String, String> {
        let document = web_sys::window()
            .and_then(|window| window.document())
            .ok_or("no document")?;
        let parts = js_sys::Array::of1(&JsValue::from_str(text));
        let options = web_sys::BlobPropertyBag::new();
        options.set_type("text/plain;charset=utf-8");
        let blob =
            web_sys::Blob::new_with_str_sequence_and_options(&parts, &options).map_err(js)?;
        let url = web_sys::Url::create_object_url_with_blob(&blob).map_err(js)?;
        let anchor: web_sys::HtmlAnchorElement = document
            .create_element("a")
            .map_err(js)?
            .dyn_into()
            .map_err(|_| "not an anchor")?;
        let name = path.rsplit('/').next().unwrap_or(path);
        anchor.set_href(&url);
        anchor.set_download(name);
        anchor.click();
        web_sys::Url::revoke_object_url(&url).map_err(js)?;
        Ok(format!("{name} (downloaded)"))
    }

    pub fn read_json_dir(dir: &str) -> Vec<(String, Result<String, String>)> {
        let Ok(storage) = storage() else {
            return Vec::new();
        };
        let prefix = format!("{}/", dir.trim_end_matches('/'));
        let len = storage.length().unwrap_or(0);
        let mut keys: Vec<String> = (0..len)
            .filter_map(|i| storage.key(i).ok().flatten())
            .filter(|key| {
                key.strip_prefix(&prefix)
                    .is_some_and(|name| !name.contains('/') && name.ends_with(".json"))
            })
            .collect();
        keys.sort();
        keys.into_iter()
            .map(|key| {
                let text = read(&key);
                (key, text)
            })
            .collect()
    }
}
