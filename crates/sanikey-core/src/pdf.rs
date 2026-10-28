//! Estrazione del testo digitale PDF e OCR per pagine scansionate.

use crate::metadata::DocumentRecord;
use pdfium_render::prelude::{PdfPageObjectCommon, PdfPageObjectsCommon, PdfRenderConfig, Pdfium};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

const MIN_PDF_TEXT_CHARACTERS: usize = 40;
const SCANNED_PDF_PAGE_IMAGE_COVERAGE: f32 = 0.90;
const OCR_TARGET_WIDTH: i32 = 2550;

/// Intervallo di testo estratto da una pagina PDF, con offset Unicode.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TextPageSpan {
    /// Numero pagina a partire da 1.
    pub page_number: usize,
    /// Offset iniziale nel testo concatenato, in valori Unicode.
    pub character_start: usize,
    /// Offset finale esclusivo nel testo concatenato, in valori Unicode.
    pub character_end: usize,
}

/// Testo estratto e provenienza per un documento PDF.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExtractedText {
    /// Identificatore stabile del documento originale.
    pub document_id: String,
    /// Testo delle pagine separate da newline.
    pub text: String,
    /// Avvisi su estrazione incompleta o non disponibile.
    pub warnings: Vec<String>,
    /// Intervalli del testo associati alle pagine originali.
    pub page_spans: Vec<TextPageSpan>,
}

/// Controlli opzionali applicati durante l'estrazione PDF.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct PdfExtractionOptions {
    /// Ripete l'OCR sui PDF scansionati anche quando il testo digitale è sufficiente.
    pub force_pdf_ocr: bool,
}

/// Estrae il testo PDF e associa ogni intervallo alla pagina originale.
///
/// Se il testo digitale non raggiunge 40 caratteri non whitespace, prova OCR
/// sulle pagine PDF. La forzatura OCR è limitata ai documenti in cui ogni
/// pagina è coperta per almeno il 90% da una singola immagine. Non esegue OCR
/// diretto sui file immagine sorgente né modifica il PDF.
pub fn extract_pdf_text(document: &DocumentRecord) -> ExtractedText {
    extract_pdf_text_with_options(document, PdfExtractionOptions::default())
}

/// Estrae il testo PDF applicando controlli espliciti di forzatura OCR.
pub fn extract_pdf_text_with_options(
    document: &DocumentRecord,
    options: PdfExtractionOptions,
) -> ExtractedText {
    let mut digital = extract_digital_pdf_text(document);
    let sufficient_text = has_sufficient_pdf_text(&digital.text);
    if sufficient_text && !options.force_pdf_ocr {
        return digital;
    }

    let ocr = match extract_pdf_ocr_text(
        &document.document_id,
        &document.path,
        sufficient_text && options.force_pdf_ocr,
    ) {
        Ok(ocr) => ocr,
        Err(error) => {
            digital.warnings.retain(|warning| {
                warning != "il testo PDF digitale è insufficiente e l'OCR non è ancora disponibile"
            });
            if !sufficient_text {
                digital.warnings.push(format!(
                    "testo PDF insufficiente; OCR non riuscito: {error}"
                ));
            } else if options.force_pdf_ocr {
                digital
                    .warnings
                    .push(format!("OCR PDF forzato non riuscito: {error}"));
            }
            return digital;
        }
    };

    if !ocr.attempted {
        return digital;
    }

    let mut result = ocr.extracted;
    result.warnings = digital
        .warnings
        .into_iter()
        .filter(|warning| {
            warning != "il testo PDF digitale è insufficiente e l'OCR non è ancora disponibile"
        })
        .collect();
    result.warnings.extend(ocr.warnings);
    result
}

fn extract_digital_pdf_text(document: &DocumentRecord) -> ExtractedText {
    let mut result = ExtractedText {
        document_id: document.document_id.clone(),
        text: String::new(),
        warnings: Vec::new(),
        page_spans: Vec::new(),
    };
    let pdf = match lopdf::Document::load(&document.path) {
        Ok(pdf) => pdf,
        Err(error) => {
            result
                .warnings
                .push(format!("impossibile leggere il PDF digitale: {error}"));
            return result;
        }
    };
    let pages = pdf.get_pages();
    if pages.is_empty() {
        result
            .warnings
            .push("il PDF non contiene pagine".to_owned());
        return result;
    }
    for (index, page_number) in pages.keys().enumerate() {
        let page_text = match pdf.extract_text(&[*page_number]) {
            Ok(text) => text,
            Err(error) => {
                result.warnings.push(format!(
                    "impossibile estrarre il testo PDF della pagina {page_number}: {error}"
                ));
                return result;
            }
        };
        if index > 0 {
            result.text.push('\n');
        }
        let start = result.text.chars().count();
        result.text.push_str(&page_text);
        let end = result.text.chars().count();
        result.page_spans.push(TextPageSpan {
            page_number: *page_number as usize,
            character_start: start,
            character_end: end,
        });
    }
    if !has_sufficient_pdf_text(&result.text) {
        result.warnings.push(
            "il testo PDF digitale è insufficiente e l'OCR non è ancora disponibile".to_owned(),
        );
    }
    result
}

fn has_sufficient_pdf_text(text: &str) -> bool {
    text.chars()
        .filter(|character| !character.is_whitespace())
        .count()
        >= MIN_PDF_TEXT_CHARACTERS
}

struct OcrExtraction {
    extracted: ExtractedText,
    warnings: Vec<String>,
    attempted: bool,
}

fn extract_pdf_ocr_text(
    document_id: &str,
    path: &Path,
    require_scanned_pdf: bool,
) -> Result<OcrExtraction, String> {
    let pdfium = bind_pdfium()?;
    let document = pdfium
        .load_pdf_from_file(path, None)
        .map_err(|error| format!("PDFium non può aprire il PDF: {error}"))?;
    if document.pages().is_empty() {
        return Err("il PDF non contiene pagine".to_owned());
    }

    for (index, page) in document.pages().iter().enumerate() {
        let page_area = page.width().value * page.height().value;
        if page_area <= 0.0 {
            return Err(format!("area non valida nella pagina {}", index + 1));
        }
        let largest_image_area = page
            .objects()
            .iter()
            .filter_map(|object| {
                object.as_image_object().and_then(|image| {
                    let bounds = image.bounds().ok()?;
                    let left = bounds.left().value.max(0.0);
                    let right = bounds.right().value.min(page.width().value);
                    let bottom = bounds.bottom().value.max(0.0);
                    let top = bounds.top().value.min(page.height().value);
                    let width = (right - left).max(0.0);
                    let height = (top - bottom).max(0.0);
                    (width > 0.0 && height > 0.0).then_some(width * height)
                })
            })
            .fold(0.0_f32, f32::max);
        if require_scanned_pdf && largest_image_area / page_area < SCANNED_PDF_PAGE_IMAGE_COVERAGE {
            return Ok(OcrExtraction {
                extracted: empty_extraction(document_id),
                warnings: Vec::new(),
                attempted: false,
            });
        }
    }

    let tesseract = find_tesseract()?;
    let glossary = GlossaryFile::create()?;
    let mut extracted = empty_extraction(document_id);
    let mut warnings = Vec::new();
    for (index, page) in document.pages().iter().enumerate() {
        let bitmap = page
            .render_with_config(
                &PdfRenderConfig::new()
                    .set_target_width(OCR_TARGET_WIDTH)
                    .set_maximum_height(3500),
            )
            .map_err(|error| format!("render pagina {} non riuscito: {error}", index + 1))?;
        let ppm = rgba_to_ppm(&bitmap.as_rgba_bytes(), bitmap.width(), bitmap.height())?;
        let output = run_tesseract(&tesseract, &glossary.path, &ppm, index + 1)?;
        let page_text = String::from_utf8_lossy(&output.stdout).trim().to_owned();
        if index > 0 {
            extracted.text.push('\n');
        }
        let start = extracted.text.chars().count();
        extracted.text.push_str(&page_text);
        let end = extracted.text.chars().count();
        extracted.page_spans.push(TextPageSpan {
            page_number: index + 1,
            character_start: start,
            character_end: end,
        });
    }
    if !has_sufficient_pdf_text(&extracted.text) {
        warnings.push("l'OCR non ha restituito testo sufficiente".to_owned());
    }
    Ok(OcrExtraction {
        extracted,
        warnings,
        attempted: true,
    })
}

fn empty_extraction(document_id: &str) -> ExtractedText {
    ExtractedText {
        document_id: document_id.to_owned(),
        text: String::new(),
        warnings: Vec::new(),
        page_spans: Vec::new(),
    }
}

fn bind_pdfium() -> Result<Pdfium, String> {
    let library_name = Pdfium::pdfium_platform_library_name_at_path("");
    let mut candidates = Vec::new();
    if let Some(explicit) = std::env::var_os("SANIKEY_PDFIUM_LIBRARY") {
        candidates.push(PathBuf::from(explicit));
    }
    if let Ok(executable) = std::env::current_exe()
        && let Some(directory) = executable.parent()
    {
        candidates.push(directory.join(&library_name));
    }
    for candidate in candidates {
        match Pdfium::bind_to_library(&candidate) {
            Ok(bindings) => return Ok(Pdfium::new(bindings)),
            Err(pdfium_render::prelude::PdfiumError::PdfiumLibraryBindingsAlreadyInitialized) => {
                return Ok(Pdfium::default());
            }
            Err(_) => {}
        }
    }
    match Pdfium::bind_to_system_library() {
        Ok(bindings) => Ok(Pdfium::new(bindings)),
        Err(pdfium_render::prelude::PdfiumError::PdfiumLibraryBindingsAlreadyInitialized) => {
            Ok(Pdfium::default())
        }
        Err(error) => Err(format!(
            "libreria PDFium non disponibile; collocarla accanto all'eseguibile o impostare SANIKEY_PDFIUM_LIBRARY: {error}"
        )),
    }
}

fn find_tesseract() -> Result<PathBuf, String> {
    if let Some(explicit) = std::env::var_os("SANIKEY_TESSERACT") {
        let path = PathBuf::from(explicit);
        if path.is_file() {
            return Ok(path);
        }
        return Err("SANIKEY_TESSERACT non indica un eseguibile disponibile".to_owned());
    }
    let search_path = std::env::var_os("PATH").unwrap_or_default();
    for directory in std::env::split_paths(&search_path) {
        let executable = directory.join(if cfg!(windows) {
            "tesseract.exe"
        } else {
            "tesseract"
        });
        if executable.is_file() {
            return Ok(executable);
        }
    }
    Err("eseguibile Tesseract non disponibile nel PATH".to_owned())
}

fn rgba_to_ppm(rgba: &[u8], width: i32, height: i32) -> Result<Vec<u8>, String> {
    if width <= 0 || height <= 0 {
        return Err("bitmap OCR con dimensioni non valide".to_owned());
    }
    let expected = usize::try_from(width)
        .ok()
        .and_then(|width| {
            usize::try_from(height)
                .ok()
                .and_then(|height| width.checked_mul(height))
        })
        .and_then(|pixels| pixels.checked_mul(4))
        .ok_or_else(|| "dimensioni bitmap OCR eccessive".to_owned())?;
    if rgba.len() != expected {
        return Err("dimensione bitmap OCR incoerente".to_owned());
    }
    let mut ppm = format!("P6\n{width} {height}\n255\n").into_bytes();
    ppm.reserve(expected / 4 * 3);
    let (pixels, remainder) = rgba.as_chunks::<4>();
    debug_assert!(remainder.is_empty());
    for pixel in pixels {
        ppm.extend_from_slice(&pixel[..3]);
    }
    Ok(ppm)
}

fn run_tesseract(
    executable: &Path,
    glossary: &Path,
    ppm: &[u8],
    page_number: usize,
) -> Result<std::process::Output, String> {
    let mut child = Command::new(executable)
        .args(["stdin", "stdout", "--dpi", "300", "--user-words"])
        .arg(glossary)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|error| format!("avvio di Tesseract non riuscito: {error}"))?;
    let mut stdin = child
        .stdin
        .take()
        .ok_or_else(|| "input di Tesseract non disponibile".to_owned())?;
    if let Err(error) = stdin.write_all(ppm) {
        drop(stdin);
        let _ = child.wait();
        return Err(format!(
            "invio pagina {page_number} a Tesseract non riuscito: {error}"
        ));
    }
    drop(stdin);
    let output = child
        .wait_with_output()
        .map_err(|error| format!("attesa di Tesseract non riuscita: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "Tesseract fallito sulla pagina {page_number} con stato {}",
            output.status
        ));
    }
    Ok(output)
}

struct GlossaryFile {
    path: PathBuf,
}

impl GlossaryFile {
    fn create() -> Result<Self, String> {
        static NEXT_FILE: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let bytes = include_bytes!("../assets/ocr/tesseract-medical-it.user-words");
        for _ in 0..32 {
            let sequence = NEXT_FILE.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            let path = std::env::temp_dir().join(format!(
                "sanikey-ocr-glossary-{}-{sequence}.txt",
                std::process::id()
            ));
            let mut options = std::fs::OpenOptions::new();
            options.write(true).create_new(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                options.mode(0o600);
            }
            match options.open(&path) {
                Ok(mut file) => {
                    if let Err(error) = file.write_all(bytes) {
                        let _ = std::fs::remove_file(&path);
                        return Err(format!("scrittura glossario OCR non riuscita: {error}"));
                    }
                    return Ok(Self { path });
                }
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(error) => return Err(format!("creazione glossario OCR non riuscita: {error}")),
            }
        }
        Err("impossibile creare un file temporaneo per il glossario OCR".to_owned())
    }
}

impl Drop for GlossaryFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

/// Restituisce il numero di pagine quando il PDF è leggibile.
pub fn pdf_page_count(document: &DocumentRecord) -> Option<usize> {
    lopdf::Document::load(&document.path)
        .ok()
        .map(|pdf| pdf.get_pages().len())
}

#[cfg(test)]
mod tests;

#[cfg(test)]
pub(crate) mod test_support {
    use lopdf::{
        Document, Object, Stream,
        content::{Content, Operation},
        dictionary,
    };

    pub(crate) fn synthetic_pdf(page_texts: &[&str]) -> Vec<u8> {
        let mut document = Document::with_version("1.5");
        let pages_id = document.new_object_id();
        let font_id = document.add_object(dictionary! {
            "Type" => "Font",
            "Subtype" => "Type1",
            "BaseFont" => "Helvetica",
            "Encoding" => "WinAnsiEncoding",
        });
        let resources_id = document.add_object(dictionary! {
            "Font" => dictionary! { "F1" => font_id },
        });
        let mut page_ids = Vec::new();
        for text in page_texts {
            let content = Content {
                operations: vec![
                    Operation::new("BT", vec![]),
                    Operation::new("Tf", vec!["F1".into(), 12.into()]),
                    Operation::new("Td", vec![50.into(), 700.into()]),
                    Operation::new(
                        "Tj",
                        vec![Object::string_literal(
                            text.chars()
                                .map(|character| {
                                    u8::try_from(u32::from(character))
                                        .expect("fixture PDF sintetica in Latin-1")
                                })
                                .collect::<Vec<_>>(),
                        )],
                    ),
                    Operation::new("ET", vec![]),
                ],
            };
            let content_id = document.add_object(Stream::new(
                dictionary! {},
                content.encode().expect("contenuto PDF sintetico"),
            ));
            let page_id = document.add_object(dictionary! {
                "Type" => "Page",
                "Parent" => pages_id,
                "Contents" => content_id,
                "Resources" => resources_id,
                "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
            });
            page_ids.push(Object::Reference(page_id));
        }
        document.objects.insert(
            pages_id,
            Object::Dictionary(dictionary! {
                "Type" => "Pages",
                "Kids" => page_ids,
                "Count" => page_texts.len() as i64,
            }),
        );
        let catalog_id = document.add_object(dictionary! {
            "Type" => "Catalog",
            "Pages" => pages_id,
        });
        document.trailer.set("Root", catalog_id);
        let mut bytes = Vec::new();
        document
            .save_modern(&mut bytes)
            .expect("PDF sintetico valido");
        bytes
    }

    pub(crate) fn synthetic_scanned_pdf(text: &str) -> Vec<u8> {
        synthetic_scanned_pdf_with_digital_text(text, None)
    }

    pub(crate) fn synthetic_scanned_pdf_with_digital_text(
        text: &str,
        digital_text: Option<&str>,
    ) -> Vec<u8> {
        use lopdf::{
            Object, Stream,
            content::{Content, Operation},
        };

        const WIDTH: usize = 600;
        const HEIGHT: usize = 800;
        let mut pixels = vec![255_u8; WIDTH * HEIGHT * 3];
        let mut x = 70_usize;
        let mut top = 300_usize;
        for character in text.chars() {
            if character == ' ' {
                if x + 20 > WIDTH - 70 {
                    x = 70;
                    top += 100;
                    continue;
                }
                x += 20;
                continue;
            }
            let glyph = glyph(character).expect("glyph della fixture sintetica");
            for (row, pattern) in glyph.iter().enumerate() {
                for (column, pixel) in pattern.bytes().enumerate() {
                    if pixel != b'1' {
                        continue;
                    }
                    for dy in 0..4 {
                        for dx in 0..4 {
                            let offset = ((top + row * 4 + dy) * WIDTH + x + column * 4 + dx) * 3;
                            pixels[offset..offset + 3].fill(0);
                        }
                    }
                }
            }
            x += 28;
        }

        let mut document = Document::with_version("1.5");
        let pages_id = document.new_object_id();
        let image_id = document.add_object(Stream::new(
            dictionary! {
                "Type" => "XObject",
                "Subtype" => "Image",
                "Width" => WIDTH as i64,
                "Height" => HEIGHT as i64,
                "ColorSpace" => "DeviceRGB",
                "BitsPerComponent" => 8,
            },
            pixels,
        ));
        let content = Content {
            operations: {
                let mut operations = vec![
                    Operation::new("q", vec![]),
                    Operation::new(
                        "cm",
                        vec![
                            612.into(),
                            0.into(),
                            0.into(),
                            792.into(),
                            0.into(),
                            0.into(),
                        ],
                    ),
                    Operation::new("Do", vec![Object::Name(b"Scan".to_vec())]),
                    Operation::new("Q", vec![]),
                ];
                if let Some(digital_text) = digital_text {
                    operations.extend([
                        Operation::new("BT", vec![]),
                        Operation::new("Tf", vec!["F1".into(), 12.into()]),
                        Operation::new("Td", vec![20.into(), 20.into()]),
                        Operation::new(
                            "Tj",
                            vec![Object::string_literal(
                                digital_text.bytes().collect::<Vec<_>>(),
                            )],
                        ),
                        Operation::new("ET", vec![]),
                    ]);
                }
                operations
            },
        };
        let content_id = document.add_object(Stream::new(
            dictionary! {},
            content.encode().expect("contenuto PDF scansionato"),
        ));
        let mut resources = dictionary! {
            "XObject" => dictionary! { "Scan" => image_id },
        };
        if digital_text.is_some() {
            let font_id = document.add_object(dictionary! {
                "Type" => "Font",
                "Subtype" => "Type1",
                "BaseFont" => "Helvetica",
                "Encoding" => "WinAnsiEncoding",
            });
            resources.set("Font", dictionary! { "F1" => font_id });
        }
        let page_id = document.add_object(dictionary! {
            "Type" => "Page",
            "Parent" => pages_id,
            "Contents" => content_id,
            "Resources" => resources,
            "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
        });
        document.objects.insert(
            pages_id,
            Object::Dictionary(dictionary! {
                "Type" => "Pages",
                "Kids" => vec![Object::Reference(page_id)],
                "Count" => 1,
            }),
        );
        let catalog_id = document.add_object(dictionary! {
            "Type" => "Catalog",
            "Pages" => pages_id,
        });
        document.trailer.set("Root", catalog_id);
        let mut bytes = Vec::new();
        document
            .save_modern(&mut bytes)
            .expect("PDF scansionato sintetico valido");
        bytes
    }

    fn glyph(character: char) -> Option<[&'static str; 7]> {
        Some(match character {
            'A' => [
                "01110", "10001", "10001", "11111", "10001", "10001", "10001",
            ],
            'C' => [
                "01111", "10000", "10000", "10000", "10000", "10000", "01111",
            ],
            'E' => [
                "11111", "10000", "10000", "11110", "10000", "10000", "11111",
            ],
            'H' => [
                "10001", "10001", "10001", "11111", "10001", "10001", "10001",
            ],
            'I' => [
                "11111", "00100", "00100", "00100", "00100", "00100", "11111",
            ],
            'N' => [
                "10001", "11001", "11001", "10101", "10011", "10011", "10001",
            ],
            'O' => [
                "01110", "10001", "10001", "10001", "10001", "10001", "01110",
            ],
            'R' => [
                "11110", "10001", "10001", "11110", "10100", "10010", "10001",
            ],
            'S' => [
                "01111", "10000", "10000", "01110", "00001", "00001", "11110",
            ],
            'T' => [
                "11111", "00100", "00100", "00100", "00100", "00100", "00100",
            ],
            'Y' => [
                "10001", "10001", "01010", "00100", "00100", "00100", "00100",
            ],
            _ => return None,
        })
    }
}
