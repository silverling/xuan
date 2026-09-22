use std::path::PathBuf;

use anyhow::{Context, Result, ensure};
use image::RgbaImage;
use url::Url;
use xuan::{document::validate_size, io};

use super::{EditorApp, Layer, Point};

pub(super) enum ClipboardContent {
    Image(RgbaImage),
    Files(Vec<PathBuf>),
    Empty,
    Unavailable,
}

/// File managers can offer URI lists, GNOME copy/cut lists, or absolute paths.
fn file_paths(text: &str) -> Option<Vec<PathBuf>> {
    let mut paths = Vec::new();
    for line in text.lines().map(str::trim) {
        if line.is_empty()
            || line.starts_with('#')
            || matches!(line, "copy" | "cut" | "x-special/nautilus-clipboard")
        {
            continue;
        }
        let path = if line.starts_with("file:") {
            Url::parse(line).ok()?.to_file_path().ok()?
        } else {
            PathBuf::from(line)
        };
        if !path.is_absolute() {
            return None;
        }
        if !paths.contains(&path) {
            paths.push(path);
        }
    }
    (!paths.is_empty()).then_some(paths)
}

fn native_file_paths(paths: Vec<PathBuf>) -> Result<Vec<PathBuf>> {
    paths
        .into_iter()
        .map(|mut path| {
            // arboard 3.6 decodes URIs but retains CRLF terminators and localhost.
            if !path.exists()
                && let Some(text) = path.to_str().and_then(|text| text.strip_suffix('\r'))
            {
                path = PathBuf::from(text);
            }
            #[cfg(unix)]
            if let Ok(local) = path.strip_prefix("localhost") {
                path = PathBuf::from("/").join(local);
            }
            ensure!(path.is_absolute(), "Only local image files can be pasted");
            Ok(path)
        })
        .collect()
}

fn read_clipboard(
    clipboard: Option<&mut arboard::Clipboard>,
    text: Option<&str>,
) -> Result<ClipboardContent> {
    if let Some(paths) = text.and_then(file_paths) {
        return Ok(ClipboardContent::Files(paths));
    }
    let Some(clipboard) = clipboard else {
        return Ok(if text.is_some_and(|text| !text.is_empty()) {
            ClipboardContent::Empty
        } else {
            ClipboardContent::Unavailable
        });
    };

    // Prefer original files over any preview image offered by a file manager.
    if let Ok(paths) = clipboard.get().file_list()
        && !paths.is_empty()
    {
        return Ok(ClipboardContent::Files(native_file_paths(paths)?));
    }
    match clipboard.get_image() {
        Ok(data) => {
            let width = u32::try_from(data.width)?;
            let height = u32::try_from(data.height)?;
            validate_size(width, height)?;
            let pixels = RgbaImage::from_raw(width, height, data.bytes.into_owned())
                .context("Invalid clipboard image pixels")?;
            return Ok(ClipboardContent::Image(pixels));
        }
        Err(arboard::Error::ContentNotAvailable) => {}
        Err(error) => return Err(error).context("Could not read the clipboard image"),
    }
    if let Ok(text) = clipboard.get_text()
        && let Some(paths) = file_paths(&text)
    {
        return Ok(ClipboardContent::Files(paths));
    }
    Ok(ClipboardContent::Empty)
}

impl EditorApp {
    pub(super) fn connect_clipboard(&mut self) {
        if self.system_clipboard.is_none() {
            // Keep the owner alive so copied pixels survive without a clipboard manager.
            self.system_clipboard = arboard::Clipboard::new().ok();
        }
    }

    pub(super) fn paste_clipboard(&mut self, text: Option<&str>) {
        self.connect_clipboard();
        match read_clipboard(self.system_clipboard.as_mut(), text) {
            Ok(content) => self.paste_content(content),
            Err(error) => {
                self.clipboard = None;
                self.error = Some(format!("Could not paste\n\n{error:#}"));
            }
        }
    }

    pub(super) fn paste_content(&mut self, content: ClipboardContent) {
        let images = match content {
            ClipboardContent::Image(pixels) => {
                let point = self
                    .clipboard
                    .as_ref()
                    .filter(|(cached, _)| *cached == pixels)
                    .map(|(_, point)| *point);
                if point.is_none() {
                    self.clipboard = None;
                }
                vec![("Pasted image".to_owned(), pixels, point)]
            }
            ClipboardContent::Files(paths) => {
                self.clipboard = None;
                if paths.iter().any(|p| xuan::raw::is_raw(p)) {
                    for path in paths {
                        self.open_path(&path, true);
                    }
                    return;
                }
                let images: Result<Vec<_>> = paths
                    .iter()
                    .map(|path| {
                        let pixels = io::import_image(path)
                            .with_context(|| format!("Could not paste {}", path.display()))?;
                        let name = path
                            .file_stem()
                            .unwrap_or_default()
                            .to_string_lossy()
                            .into_owned();
                        Ok((name, pixels, None))
                    })
                    .collect();
                match images {
                    Ok(images) => images,
                    Err(error) => {
                        self.error = Some(format!("{error:#}"));
                        return;
                    }
                }
            }
            ClipboardContent::Unavailable => match self.clipboard.clone() {
                Some((pixels, point)) => vec![("Pasted image".into(), pixels, Some(point))],
                None => {
                    self.status = "The system clipboard is unavailable".into();
                    return;
                }
            },
            ClipboardContent::Empty => {
                self.clipboard = None;
                self.status = "The clipboard does not contain an image or image file".into();
                return;
            }
        };
        if images.is_empty() {
            return;
        }
        if self.sessions.is_empty() {
            self.dimensions = [
                images
                    .iter()
                    .map(|(_, pixels, _)| pixels.width())
                    .max()
                    .unwrap(),
                images
                    .iter()
                    .map(|(_, pixels, _)| pixels.height())
                    .max()
                    .unwrap(),
            ];
            self.new_document();
        }
        self.edit("Paste", |doc| {
            for (name, pixels, point) in images {
                let mut layer = Layer::image(name, pixels);
                let point = point.unwrap_or_else(|| {
                    Point::new(
                        (doc.width as f32 - layer.transform.width) * 0.5,
                        (doc.height as f32 - layer.transform.height) * 0.5,
                    )
                });
                layer.transform.x = point.x;
                layer.transform.y = point.y;
                doc.insert(layer);
            }
            doc.validate()
        });
        self.mask_target = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    #[test]
    fn file_lists_decode_local_uris_and_file_manager_headers() {
        let paths = file_paths(
            "# copied images\r\nx-special/nautilus-clipboard\r\ncopy\r\nfile:///tmp/a%20b.png\r\nfile://localhost/tmp/%E5%9B%BE%E7%89%87%23.png\r\nfile:///tmp/a%20b.png\r\n",
        )
        .unwrap();
        assert_eq!(
            paths,
            [
                PathBuf::from("/tmp/a b.png"),
                PathBuf::from("/tmp/图片#.png")
            ]
        );
        assert_eq!(
            file_paths("cut\n/tmp/a b.png\n/tmp/c.jpg").unwrap(),
            [PathBuf::from("/tmp/a b.png"), PathBuf::from("/tmp/c.jpg")]
        );
    }

    #[test]
    fn file_lists_reject_unrelated_text() {
        for text in [
            "",
            "copy",
            "notes",
            "relative.png",
            "https://example.com/a.png",
            "/tmp/a.png\nnotes",
        ] {
            assert!(file_paths(text).is_none(), "{text}");
        }
        assert!(matches!(
            read_clipboard(None, Some("notes")).unwrap(),
            ClipboardContent::Empty
        ));
        assert!(matches!(
            read_clipboard(None, Some("")).unwrap(),
            ClipboardContent::Unavailable
        ));
    }

    #[cfg(unix)]
    #[test]
    fn unix_file_lists_normalize_native_uris_and_reject_remote_hosts() {
        assert!(file_paths("file://remote-host/tmp/a.png").is_none());
        assert!(matches!(
            read_clipboard(None, Some("file:///tmp/a.png")).unwrap(),
            ClipboardContent::Files(_)
        ));
        assert_eq!(
            native_file_paths(vec![
                PathBuf::from("/tmp/a.png\r"),
                PathBuf::from("localhost/tmp/b.png")
            ])
            .unwrap(),
            [PathBuf::from("/tmp/a.png"), PathBuf::from("/tmp/b.png")]
        );
        assert!(native_file_paths(vec![PathBuf::from("remote-host/tmp/a.png")]).is_err());
    }

    #[cfg(windows)]
    #[test]
    fn windows_file_lists_accept_drive_paths_file_urls_and_unc_paths() {
        let expected = [
            PathBuf::from(r"C:\Pictures\a b.png"),
            PathBuf::from(r"C:\Pictures\图片#.png"),
        ];
        assert_eq!(
            file_paths("file:///C:/Pictures/a%20b.png\r\nfile://localhost/C:/Pictures/%E5%9B%BE%E7%89%87%23.png\r\nfile:///C:/Pictures/a%20b.png").unwrap(),
            expected
        );
        assert_eq!(
            file_paths("C:\\Pictures\\a b.png\r\nC:\\Pictures\\图片#.png").unwrap(),
            expected
        );
        assert_eq!(native_file_paths(expected.to_vec()).unwrap(), expected);
        assert!(matches!(
            read_clipboard(None, Some("file:///C:/Pictures/a.png")).unwrap(),
            ClipboardContent::Files(_)
        ));
        let network_path = PathBuf::from(r"\\server\share\a.png");
        assert_eq!(
            file_paths("file://server/share/a.png").unwrap(),
            std::slice::from_ref(&network_path)
        );
        assert_eq!(
            native_file_paths(vec![network_path.clone()]).unwrap(),
            [network_path]
        );
        for path in [r"C:a.png", r"\Pictures\a.png", "relative.png"] {
            assert!(file_paths(path).is_none(), "{path}");
            assert!(native_file_paths(vec![PathBuf::from(path)]).is_err());
        }
    }
}
