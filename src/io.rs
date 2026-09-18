use std::{
    collections::HashSet,
    fs::{self, File},
    io::{Cursor, Read, Write},
    path::Path,
    sync::Arc,
};

use anyhow::{Context, Result, bail, ensure};
use image::{DynamicImage, ImageFormat, ImageReader, RgbaImage};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;
use zip::{ZipArchive, ZipWriter, write::SimpleFileOptions};

use crate::{
    blend::BlendMode,
    document::{Adjustment, Document, Layer, MAX_PIXELS, Mask, Point, Transform, validate_size},
    render,
};

const MAX_MANIFEST: u64 = 4 * 1024 * 1024;
const MAX_ASSET: u64 = 512 * 1024 * 1024;

#[derive(Serialize, Deserialize)]
struct Manifest {
    format: String,
    version: u32,
    document: Document,
    pixel_layers: HashSet<Uuid>,
}

fn encode_png(image: &DynamicImage) -> Result<Vec<u8>> {
    let mut encoded = Cursor::new(Vec::new());
    image.write_to(&mut encoded, ImageFormat::Png)?;
    Ok(encoded.into_inner())
}

fn decode_image(bytes: Vec<u8>, used: &mut u64) -> Result<DynamicImage> {
    ensure!(bytes.len() as u64 <= MAX_ASSET, "Image file is too large");
    let mut reader = ImageReader::new(Cursor::new(bytes)).with_guessed_format()?;
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(30_000);
    limits.max_image_height = Some(30_000);
    limits.max_alloc = Some(MAX_PIXELS * 8);
    reader.limits(limits);
    let mut decoder = reader.into_decoder()?;
    use image::ImageDecoder;
    let (width, height) = decoder.dimensions();
    validate_size(width, height)?;
    *used += u64::from(width) * u64::from(height);
    ensure!(
        *used <= MAX_PIXELS,
        "Project exceeds 100 megapixels of source images"
    );
    let orientation = decoder.orientation()?;
    let mut image = DynamicImage::from_decoder(decoder)?;
    image.apply_orientation(orientation);
    Ok(image)
}

pub fn import_image(path: &Path) -> Result<RgbaImage> {
    let metadata = fs::metadata(path).with_context(|| format!("Cannot read {}", path.display()))?;
    ensure!(metadata.len() <= MAX_ASSET, "Image exceeds 512 MiB");
    Ok(decode_image(fs::read(path)?, &mut 0)?.to_rgba8())
}

/// Persist a complete sibling temporary file, then atomically replace the destination.
pub fn save(document: &Document, path: &Path) -> Result<()> {
    document.validate()?;
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let mut temporary = tempfile::NamedTempFile::new_in(parent)?;
    {
        let mut archive = ZipWriter::new(temporary.as_file_mut());
        let options =
            SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
        let manifest = Manifest {
            format: "org.xuan.project".into(),
            version: 1,
            document: document.clone(),
            pixel_layers: document
                .layers
                .iter()
                .filter(|l| l.pixels.is_some())
                .map(|l| l.id)
                .collect(),
        };
        let json = serde_json::to_vec_pretty(&manifest)?;
        ensure!(
            json.len() as u64 <= MAX_MANIFEST,
            "Project metadata exceeds 4 MiB"
        );
        archive.start_file("manifest.json", options)?;
        archive.write_all(&json)?;
        for layer in &document.layers {
            if let Some(pixels) = &layer.pixels {
                archive.start_file(format!("images/{}.png", layer.id), options)?;
                archive.write_all(&encode_png(&DynamicImage::ImageRgba8((**pixels).clone()))?)?;
            }
            if let Some(mask) = &layer.mask {
                archive.start_file(format!("images/{}.mask.png", layer.id), options)?;
                archive.write_all(&encode_png(&DynamicImage::ImageLuma8(
                    (*mask.pixels).clone(),
                ))?)?;
            }
        }
        archive.finish()?;
    }
    temporary.as_file().sync_all()?;
    temporary.persist(path).map_err(|error| error.error)?;
    File::open(parent)?.sync_all()?;
    Ok(())
}

fn zip_read(archive: &mut ZipArchive<File>, name: &str, limit: u64) -> Result<Vec<u8>> {
    let file = archive
        .by_name(name)
        .with_context(|| format!("Missing project asset: {name}"))?;
    ensure!(file.size() <= limit, "Project asset exceeds size limit");
    let mut bytes = Vec::new();
    file.take(limit + 1).read_to_end(&mut bytes)?;
    ensure!(
        bytes.len() as u64 <= limit,
        "Project asset exceeds size limit"
    );
    Ok(bytes)
}

pub fn load(path: &Path) -> Result<Document> {
    if path.is_dir() {
        return load_compositor(path);
    }
    let mut archive = ZipArchive::new(File::open(path)?)?;
    ensure!(archive.len() <= 20_001, "Too many project assets");
    let mut manifest: Manifest =
        serde_json::from_slice(&zip_read(&mut archive, "manifest.json", MAX_MANIFEST)?)?;
    ensure!(
        manifest.format == "org.xuan.project" && manifest.version == 1,
        "Unsupported xuan project version"
    );
    let mut used_pixels = 0;
    let mut used_masks = 0;
    ensure!(manifest.document.layers.len() <= 10_000, "Too many layers");
    validate_size(manifest.document.width, manifest.document.height)?;
    for layer in &mut manifest.document.layers {
        if manifest.pixel_layers.contains(&layer.id) {
            let bytes = zip_read(&mut archive, &format!("images/{}.png", layer.id), MAX_ASSET)?;
            layer.pixels = Some(Arc::new(decode_image(bytes, &mut used_pixels)?.to_rgba8()));
        }
        if let Some(mask) = &mut layer.mask {
            let bytes = zip_read(
                &mut archive,
                &format!("images/{}.mask.png", layer.id),
                MAX_ASSET,
            )?;
            mask.pixels = Arc::new(decode_image(bytes, &mut used_masks)?.to_luma8());
        }
    }
    ensure!(
        manifest
            .pixel_layers
            .iter()
            .all(|id| manifest.document.layers.iter().any(|l| l.id == *id)),
        "Unreferenced pixel layer metadata"
    );
    manifest.document.selected = manifest.document.active.into_iter().collect();
    manifest.document.validate()?;
    Ok(manifest.document)
}

fn package_read(root: &Path, relative: &Path, limit: u64) -> Result<Vec<u8>> {
    let root = root.canonicalize()?;
    let file = root.join(relative);
    let metadata = fs::symlink_metadata(&file)?;
    ensure!(
        metadata.is_file() && !metadata.file_type().is_symlink() && metadata.len() <= limit,
        "Unsafe or oversized project asset"
    );
    ensure!(
        file.canonicalize()?.starts_with(&root),
        "Project asset escapes its package"
    );
    let mut bytes = Vec::new();
    File::open(file)?.take(limit + 1).read_to_end(&mut bytes)?;
    ensure!(
        bytes.len() as u64 <= limit,
        "Project asset exceeds size limit"
    );
    Ok(bytes)
}

fn number(value: &Value, key: &str, default: f32) -> f32 {
    value[key].as_f64().map_or(default, |v| v as f32)
}
fn identifier(value: &Value) -> Result<Option<Uuid>> {
    value
        .as_str()
        .map(Uuid::parse_str)
        .transpose()
        .map_err(Into::into)
}

fn comp_transform(value: &Value) -> Result<Transform> {
    let pair = |value: &Value, a: &str, b: &str| -> Result<(f32, f32)> {
        let (x, y) = if let Some(values) = value.as_array() {
            ensure!(values.len() == 2, "Invalid transform coordinates");
            (values[0].as_f64(), values[1].as_f64())
        } else {
            (value[a].as_f64(), value[b].as_f64())
        };
        Ok((
            x.context("Missing transform coordinate")? as f32,
            y.context("Missing transform coordinate")? as f32,
        ))
    };
    let (x, y) = pair(&value["origin"], "x", "y")?;
    let (width, height) = pair(&value["size"], "width", "height")?;
    let t = Transform {
        x,
        y,
        width,
        height,
        rotation: number(value, "rotation", 0.0),
        flip_x: value["flipX"].as_bool().unwrap_or(false),
        flip_y: value["flipY"].as_bool().unwrap_or(false),
        warp: None,
    };
    ensure!(t.valid(), "Invalid Compositor layer transform");
    Ok(t)
}

// Swift dictionaries with enum keys are encoded as alternating key/value arrays.
fn swift_dictionary_get<'a>(value: &'a Value, key: &str) -> &'a Value {
    if let Some(array) = value.as_array() {
        for pair in array.as_chunks::<2>().0 {
            if pair[0].as_str() == Some(key) {
                return &pair[1];
            }
        }
        &Value::Null
    } else {
        &value[key]
    }
}

fn comp_adjustment(value: &Value) -> Result<Adjustment> {
    let kind = value["kind"]
        .as_str()
        .context("Adjustment kind is missing")?;
    let result = match kind {
        "Hue/Saturation" => {
            let hsv = &value["hsvSettings"];
            if hsv.is_null() {
                Adjustment::HueSaturation {
                    hue: number(value, "hue", 0.0),
                    saturation: number(value, "saturation", 0.0),
                    lightness: number(value, "lightness", 0.0),
                    colorize: value["colorize"].as_bool().unwrap_or(false),
                }
            } else {
                let mut settings = crate::color::HueSettings {
                    range: crate::color::HueSettings::RANGES
                        .iter()
                        .position(|name| Some(*name) == hsv["range"].as_str())
                        .unwrap_or(0),
                    colorize: hsv["colorize"].as_bool().unwrap_or(false),
                    invert_range: hsv["invertRange"].as_bool().unwrap_or(false),
                    ..Default::default()
                };
                for (index, name) in crate::color::HueSettings::RANGES.iter().enumerate() {
                    let adjustment = swift_dictionary_get(&hsv["adjustments"], name);
                    settings.adjustments[index] = [
                        number(adjustment, "hue", 0.0),
                        number(adjustment, "saturation", 0.0),
                        number(adjustment, "lightness", 0.0),
                    ];
                    let band = swift_dictionary_get(&hsv["bands"], name);
                    if !band.is_null() {
                        settings.bands[index] = [
                            number(band, "falloffStart", 0.0),
                            number(band, "rangeStart", 0.0),
                            number(band, "rangeEnd", 360.0),
                            number(band, "falloffEnd", 360.0),
                        ];
                    }
                }
                Adjustment::HueRanges {
                    settings: Box::new(settings),
                }
            }
        }
        "Levels" => {
            let input = value["levels"]["ranges"]
                .as_array()
                .context("Missing levels ranges")?;
            ensure!(input.len() == 4, "Invalid levels ranges");
            let ranges = std::array::from_fn(|i| {
                let r = &input[i];
                [
                    number(r, "black", 0.0),
                    number(r, "gamma", 1.0),
                    number(r, "white", 255.0),
                    number(r, "outputBlack", 0.0),
                    number(r, "outputWhite", 255.0),
                ]
            });
            Adjustment::LevelsChannels { ranges }
        }
        "Curves" => {
            let input = value["curves"]["channels"]
                .as_array()
                .context("Missing curve channels")?;
            ensure!(
                input.len() == 4 && input.iter().all(|c| c.is_array()),
                "Invalid curve channels"
            );
            let channels = std::array::from_fn(|i| {
                input[i]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|p| Point::new(number(p, "x", 0.0) / 255.0, number(p, "y", 0.0) / 255.0))
                    .collect()
            });
            Adjustment::CurvesChannels { channels }
        }
        "Exposure" => {
            let settings = &value["exposureSettings"];
            Adjustment::Exposure {
                exposure: number(settings, "exposure", 0.0),
                offset: number(settings, "offset", 0.0),
                gamma: number(settings, "gamma", 1.0),
            }
        }
        "Gradient Map" => {
            let color = |key| {
                let c = &value["gradientMapSettings"][key];
                [
                    number(c, "red", if key == "shadows" { 0.0 } else { 1.0 }),
                    number(c, "green", if key == "shadows" { 0.0 } else { 1.0 }),
                    number(c, "blue", if key == "shadows" { 0.0 } else { 1.0 }),
                    1.0,
                ]
                .map(|v| (v.clamp(0.0, 1.0) * 255.0).round() as u8)
            };
            Adjustment::GradientMap {
                shadows: color(
                    if value["gradientMapSettings"]["reversed"]
                        .as_bool()
                        .unwrap_or(false)
                    {
                        "highlights"
                    } else {
                        "shadows"
                    },
                ),
                highlights: color(
                    if value["gradientMapSettings"]["reversed"]
                        .as_bool()
                        .unwrap_or(false)
                    {
                        "shadows"
                    } else {
                        "highlights"
                    },
                ),
            }
        }
        "Grain" => {
            let settings = &value["grainSettings"];
            Adjustment::Grain {
                amount: number(settings, "amount", 0.0),
                monochrome: settings["monochrome"].as_bool().unwrap_or(true),
                seed: settings["seed"].as_u64().unwrap_or(1) as u32,
            }
        }
        _ => bail!("Unsupported Compositor adjustment: {kind}"),
    };
    crate::effects::validate_adjustment(&result)?;
    Ok(result)
}

pub fn load_compositor(path: &Path) -> Result<Document> {
    let manifest: Value = serde_json::from_slice(&package_read(
        path,
        Path::new("manifest.json"),
        MAX_MANIFEST,
    )?)?;
    ensure!(
        manifest["format"] == "com.compositor.project",
        "Not a Compositor project"
    );
    let version = manifest["version"]
        .as_u64()
        .context("Project version missing")?;
    ensure!(
        (1..=7).contains(&version),
        "Unsupported Compositor project version {version}"
    );
    ensure!(
        manifest["colorSpace"].as_str().unwrap_or("sRGB") == "sRGB",
        "Unsupported color space"
    );
    let width = u32::try_from(manifest["width"].as_u64().context("Missing canvas width")?)?;
    let height = u32::try_from(
        manifest["height"]
            .as_u64()
            .context("Missing canvas height")?,
    )?;
    let mut document = Document::new(width, height)?;
    document.id = identifier(&manifest["documentID"])?.context("Missing document ID")?;
    document.resolution = number(&manifest, "resolution", 72.0);
    document.layers.clear();
    let records = manifest["layers"].as_array().context("Missing layers")?;
    ensure!(records.len() <= 10_000, "Too many layers");
    let mut image_pixels = 0;
    let mut mask_pixels = 0;
    for record in records {
        let id = identifier(&record["id"])?.context("Missing layer ID")?;
        let mut layer = Layer::blank(
            record["name"].as_str().context("Missing layer name")?,
            width,
            height,
        );
        layer.id = id;
        layer.visible = record["isVisible"].as_bool().unwrap_or(true);
        layer.transform = comp_transform(&record["transform"])?;
        layer.parent = identifier(&record["parentID"])?;
        layer.group = record["isGroup"].as_bool().unwrap_or(false);
        layer.opacity = number(record, "opacity", 1.0);
        let blend_name = record["blendMode"].as_str().unwrap_or("Normal");
        layer.blend = BlendMode::ALL
            .into_iter()
            .find(|b| b.name() == blend_name)
            .context("Unknown blend mode")?;
        layer.clip_to = identifier(&record["maskSourceID"])?;
        if let Some(name) = record["imageFile"].as_str() {
            ensure!(
                name.eq_ignore_ascii_case(&format!("{id}.png")),
                "Unsafe layer asset path"
            );
            let bytes = package_read(path, &Path::new("images").join(name), MAX_ASSET)?;
            layer.pixels = Some(Arc::new(decode_image(bytes, &mut image_pixels)?.to_rgba8()));
        }
        if let Some(name) = record["maskFile"].as_str() {
            ensure!(
                name.eq_ignore_ascii_case(&format!("{id}.mask.png")),
                "Unsafe mask asset path"
            );
            let bytes = package_read(path, &Path::new("images").join(name), MAX_ASSET)?;
            layer.mask = Some(Mask {
                pixels: Arc::new(decode_image(bytes, &mut mask_pixels)?.to_luma8()),
                enabled: record["maskEnabled"].as_bool().unwrap_or(true),
                linked: record["maskLinked"].as_bool().unwrap_or(true),
                placement: if record["maskPlacement"].is_null() {
                    None
                } else {
                    Some(comp_transform(&record["maskPlacement"])?)
                },
            });
        }
        if !record["adjustment"].is_null() {
            layer.adjustment = Some(comp_adjustment(&record["adjustment"])?);
        }
        document.layers.push(layer);
    }
    document.active = identifier(&manifest["activeLayerID"])?;
    document.selected = document.active.into_iter().collect();
    document.validate()?;
    Ok(document)
}

pub fn export(document: &Document, path: &Path, quality: u8) -> Result<()> {
    let image = render::render(document);
    let extension = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("png")
        .to_lowercase();
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let mut temporary = tempfile::NamedTempFile::new_in(parent)?;
    match extension.as_str() {
        "jpg" | "jpeg" => {
            let mut encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(
                temporary.as_file_mut(),
                quality.clamp(1, 100),
            );
            encoder.set_pixel_density(image::codecs::jpeg::PixelDensity::dpi(
                document.resolution.round() as u16,
            ));
            encoder.encode_image(&render::flatten_white(&image))?;
        }
        "png" => {
            DynamicImage::ImageRgba8(image).write_to(temporary.as_file_mut(), ImageFormat::Png)?
        }
        "tif" | "tiff" => {
            DynamicImage::ImageRgba8(image).write_to(temporary.as_file_mut(), ImageFormat::Tiff)?
        }
        "webp" => {
            DynamicImage::ImageRgba8(image).write_to(temporary.as_file_mut(), ImageFormat::WebP)?
        }
        _ => bail!("Export as PNG, JPEG, TIFF or WebP"),
    }
    temporary.as_file().sync_all()?;
    temporary.persist(path).map_err(|e| e.error)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{GrayImage, Luma, Rgba};

    #[test]
    fn imports_swift_enum_dictionaries_and_individual_color_channels() {
        let value = serde_json::json!({"kind":"Hue/Saturation", "hsvSettings": {
            "range":"Reds", "colorize":false, "invertRange":true,
            "adjustments":["Master", {"hue":5,"saturation":0,"lightness":0}, "Reds", {"hue":40,"saturation":-20,"lightness":3}],
            "bands":["Reds", {"falloffStart":310,"rangeStart":340,"rangeEnd":20,"falloffEnd":50}]
        }});
        let Adjustment::HueRanges { settings } = comp_adjustment(&value).unwrap() else {
            panic!("expected selective hue settings");
        };
        assert_eq!(settings.range, 1);
        assert_eq!(settings.adjustments[1], [40.0, -20.0, 3.0]);
        assert_eq!(settings.bands[1], [310.0, 340.0, 20.0, 50.0]);
        assert!(settings.invert_range);
        let default =
            serde_json::json!({"black":0,"gamma":1,"white":255,"outputBlack":0,"outputWhite":255});
        let mut value = serde_json::json!({"kind":"Levels","levels":{"ranges":[default,default,default,default]}});
        value["levels"]["ranges"][1]["gamma"] = serde_json::json!(1.5);
        let Adjustment::LevelsChannels { ranges } = comp_adjustment(&value).unwrap() else {
            panic!("expected channel levels");
        };
        assert_eq!(ranges[1][1], 1.5);
    }

    #[test]
    fn portable_project_round_trip_and_atomic_overwrite() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("test.xuan");
        let mut doc = Document::new(3, 2).unwrap();
        doc.layers[0].pixels = Some(Arc::new(RgbaImage::from_pixel(
            3,
            2,
            Rgba([20, 40, 80, 128]),
        )));
        doc.layers[0].mask = Some(Mask {
            pixels: Arc::new(GrayImage::from_pixel(3, 2, Luma([128]))),
            ..Mask::white()
        });
        save(&doc, &path).unwrap();
        let loaded = load(&path).unwrap();
        assert_eq!(render::render(&doc), render::render(&loaded));
        doc.layers[0].name = "Renamed".into();
        save(&doc, &path).unwrap();
        assert_eq!(load(&path).unwrap().layers[0].name, "Renamed");
        doc.width = 0;
        assert!(save(&doc, &path).is_err());
        assert_eq!(load(&path).unwrap().width, 3);
    }

    #[test]
    fn imports_swift_transform_and_rejects_path_traversal() {
        let directory = tempfile::tempdir().unwrap();
        fs::create_dir(directory.path().join("images")).unwrap();
        let id = Uuid::new_v4();
        let mut value = serde_json::json!({"format":"com.compositor.project", "version":7, "documentID":Uuid::new_v4(), "width":2, "height":2, "activeLayerID":id,
            "layers":[{"id":id,"name":"Test", "isVisible":true,"transform":{"origin":[1,2],"size":[2,2],"rotation":30,"flipX":true,"flipY":false}}]});
        let manifest = directory.path().join("manifest.json");
        fs::write(&manifest, serde_json::to_vec(&value).unwrap()).unwrap();
        let document = load_compositor(directory.path()).unwrap();
        assert_eq!(document.layers[0].transform.rotation, 30.0);
        assert!(document.layers[0].transform.flip_x);
        value["layers"][0]["imageFile"] = Value::String("../../outside.png".into());
        fs::write(manifest, serde_json::to_vec(&value).unwrap()).unwrap();
        assert!(load_compositor(directory.path()).is_err());
    }
}
