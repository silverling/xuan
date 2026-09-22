use std::fs;

use crate::{document::MAX_PIXELS, io};

const STRIPS: &[u8] = include_bytes!("fixtures/rgb-strips.heic");
const GRID: &[u8] = include_bytes!("fixtures/checker-grid.heic");

#[test]
fn imports_heif_extensions_and_detects_content() {
    let temporary = tempfile::tempdir().unwrap();
    for name in [
        "photo.heic",
        "photo.HEIC",
        "photo.heif",
        "photo.HIF",
        "photo",
        "photo.jpg",
    ] {
        let path = temporary.path().join(name);
        fs::write(&path, STRIPS).unwrap();
        let image = io::import_image(&path).unwrap();
        assert_eq!(image.dimensions(), (96, 32));
        for (x, expected) in [
            (16, [255, 0, 0, 255]),
            (48, [0, 255, 0, 255]),
            (80, [0, 0, 255, 255]),
        ] {
            for (actual, expected) in image.get_pixel(x, 16).0.into_iter().zip(expected) {
                assert!(actual.abs_diff(expected) <= 2, "{name}: incorrect color");
            }
        }
    }
}

#[test]
fn imports_primary_grid_image() {
    let mut used = 0;
    let image = super::decode(GRID, &mut used).unwrap();
    assert_eq!(image.dimensions(), (1024, 1024));
    assert_eq!(used, 1024 * 1024);
    // Sample both sides of the tile boundaries to catch missing/reordered tiles.
    for y in [32, 96, 480, 544, 992] {
        for x in [32, 96, 480, 544, 992] {
            let expected = if (x / 64 + y / 64) % 2 == 0 { 255 } else { 0 };
            let pixel = image.get_pixel(x, y);
            assert!(pixel[0].abs_diff(expected) <= 2);
            assert_eq!(pixel[3], 255);
        }
    }
}

#[test]
fn applies_heif_container_rotation_once() {
    let original = super::decode(STRIPS, &mut 0).unwrap();
    let mut bytes = STRIPS.to_vec();
    let rotation = bytes.windows(4).position(|b| b == b"irot").unwrap();
    // HEIF irot=1 means 90 degrees counterclockwise.
    bytes[rotation + 4] = 1;
    let image = super::decode(&bytes, &mut 0).unwrap();
    assert_eq!(image.dimensions(), (32, 96));
    assert_eq!(image, image::imageops::rotate270(&original));
}

#[test]
fn rejects_oversized_heif_before_decoding() {
    let size = STRIPS.windows(4).position(|b| b == b"ispe").unwrap();
    for (width, height) in [(30_001u32, 32u32), (20_000, 20_000)] {
        let mut bytes = STRIPS.to_vec();
        bytes[size + 8..size + 12].copy_from_slice(&width.to_be_bytes());
        bytes[size + 12..size + 16].copy_from_slice(&height.to_be_bytes());
        let error = super::decode(&bytes, &mut 0).unwrap_err().to_string();
        assert!(
            error.contains("Dimensions") || error.contains("100 megapixels"),
            "{error}"
        );
    }
    let mut used = MAX_PIXELS - 1;
    let error = io::decode_image(STRIPS.to_vec(), &mut used).unwrap_err();
    assert!(error.to_string().contains("Project exceeds 100 megapixels"));
    assert_eq!(used, MAX_PIXELS - 1);

    let temporary = tempfile::tempdir().unwrap();
    let path = temporary.path().join("oversized.heic");
    fs::File::create(&path)
        .unwrap()
        .set_len(io::MAX_ASSET + 1)
        .unwrap();
    assert!(
        io::import_image(&path)
            .unwrap_err()
            .to_string()
            .contains("512 MiB")
    );
}

#[test]
fn damaged_heif_returns_a_decode_error() {
    let temporary = tempfile::tempdir().unwrap();
    let path = temporary.path().join("damaged.HEIC");
    for bytes in [
        &b"not an image"[..],
        &STRIPS[..STRIPS.len() / 2],
        &STRIPS[..STRIPS.len() - 20],
    ] {
        fs::write(&path, bytes).unwrap();
        let error = io::import_image(&path).unwrap_err();
        assert!(error.to_string().contains("HEIC/HEIF"), "{error:#}");
    }
}

#[test]
fn imported_heif_pixels_survive_project_round_trip() {
    let temporary = tempfile::tempdir().unwrap();
    let path = temporary.path().join("photo.xuan");
    let pixels = super::decode(STRIPS, &mut 0).unwrap();
    let mut document = crate::document::Document::new(96, 32).unwrap();
    document.layers = vec![crate::document::Layer::image("HEIC photo", pixels.clone())];
    document.select(document.layers[0].id, false);
    io::save(&document, &path).unwrap();
    let loaded = io::load(&path).unwrap();
    assert_eq!(loaded.layers[0].pixels.as_deref(), Some(&pixels));
}
