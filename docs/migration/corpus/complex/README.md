# Casi per adattatori complessi

Questi casi sono distinti dal set minimo e vengono costruiti deterministici dai
test della baseline, al commit indicato nel manifest principale. Ogni caso
richiede un parser, provider o comando esterno specifico; gli output attesi
vengono dagli assert dei test elencati. Il corpus iniziale non duplica fixture
binari generabili dai test né dichiara provider di sistema qualificati.

| Caso | Generatore/provenienza baseline | Formato e aspettativa |
| --- | --- | --- |
| PDF digitale e scansionato | `tests/test_documents.py::test_extract_text_reads_synthetic_pdf_with_pymupdf`, `test_extract_text_reads_image_only_pdf_with_ocrmypdf` | PDF; testo digitale oppure OCR con warning/errori verificati |
| Office e fogli | `tests/test_documents.py::test_extract_text_reads_docx_documents`, `test_extract_text_reads_xlsx_workbooks`; `tests/test_rendering.py` | DOCX/XLSX e rendering; testo/derivato o warning esplicito |
| ZIP, 7z, TAR.XZ e RAR | `tests/test_containers.py::test_stage_container_documents_extracts_7z_archive`, `test_stage_container_documents_extracts_tar_xz_archive`, `test_stage_container_documents_warns_for_invalid_rar` | archivi; membri confinati nello staging, invalidi diagnosticati |
| DICOM e DICOMDIR | `tests/test_dicom.py::test_catalog_dicom_studies_groups_files_by_study_instance_uid`, `test_catalog_dicom_studies_reads_study_records_from_dicomdir` | DICOM; studio raggruppato, provenienza e link al supporto conservati |
| Copia USB | `tests/test_usb.py::test_validate_usb_rejects_tampered_checksum`, `test_rsync_interruption_stops_child_and_reports_retry` | layout directory; checksum errato o interruzione non producono successo |

Per aggiungere byte complessi al corpus portabile, congelare prima il generatore
sintetico, il suo SHA e l’output atteso. Gli assert correnti servono da
provenienza, non da sostituto di fixture autonome nel comparatore P02.03.
