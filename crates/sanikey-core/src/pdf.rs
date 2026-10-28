//! Estrazione del testo digitale PDF senza OCR.

use crate::metadata::DocumentRecord;

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

/// Estrae il testo digitale di un PDF e associa ogni intervallo alla pagina.
///
/// L'adattatore non esegue OCR e non modifica il PDF originale. Un testo
/// assente o inferiore a 40 caratteri non whitespace produce un warning,
/// lasciando al passaggio P07.02 l'eventuale OCR.
pub fn extract_pdf_text(document: &DocumentRecord) -> ExtractedText {
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
    if result
        .text
        .chars()
        .filter(|character| !character.is_whitespace())
        .count()
        < 40
    {
        result.warnings.push(
            "il testo PDF digitale è insufficiente e l'OCR non è ancora disponibile".to_owned(),
        );
    }
    result
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
}
