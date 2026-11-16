//! Fixture sintetiche per ricerca e persistenza AIFA.

use super::{
    AifaCandidate, AifaCandidateSearch, AifaClient, ConfirmedReferenceDecision,
    confirm_aifa_candidate, create_private_temp_file, download_confirmed_leaflets_with,
    leaflet_download_dates, local_leaflet_documents, lookup_candidate_search_with,
    medication_fingerprint, trace_aifa_json_request_error, update_confirmed_reference,
    write_confirmed_references,
};
use crate::logging::{Component, Level, Session};
use crate::metadata::{Medication, MedicationLeaflet, MedicationLeafletExclusion};
use serde_json::{Value, json};
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

static TEMP_SEQUENCE: AtomicUsize = AtomicUsize::new(0);

struct SearchFixture {
    responses: std::vec::IntoIter<Option<Value>>,
    requests: Vec<String>,
}

/// Restituisce risposte controllate e conserva i percorsi richiesti dal test.
impl SearchFixture {
    fn new(responses: Vec<Option<Value>>) -> Self {
        Self {
            responses: responses.into_iter(),
            requests: Vec::new(),
        }
    }

    fn fetch(&mut self, path: &str) -> Option<Value> {
        self.requests.push(path.to_owned());
        self.responses.next().flatten()
    }
}

fn candidate(title: &str, active: &[&str], form: &str, strength: &str, code: u32) -> Value {
    json!({
        "principiAttiviIt": active,
        "formaFarmaceutica": form,
        "descrizioneFormaDosaggio": strength,
        "medicinale": {
            "codiceSis": code,
            "aic6": code + 1000,
            "denominazioneMedicinale": title,
        }
    })
}

fn response_with_candidates(items: Vec<Value>) -> Value {
    json!({"data": {"content": items}})
}

fn success(payload: Value) -> Option<Value> {
    Some(payload)
}

fn assert_trace_span<'a>(records: &'a [Value], function: &str) -> (&'a Value, &'a Value) {
    let entry = records
        .iter()
        .find(|record| record["type"] == "function_enter" && record["function"] == function)
        .unwrap_or_else(|| panic!("span di ingresso assente: {function}"));
    let exit = records
        .iter()
        .find(|record| {
            record["type"] == "function_exit"
                && record["function"] == function
                && record["call_id"] == entry["call_id"]
        })
        .unwrap_or_else(|| panic!("span di uscita abbinata assente: {function}"));
    assert_eq!(
        exit["outcome"], "returned",
        "esito non restituito: {function}"
    );
    assert!(
        exit["duration_ms"].is_u64(),
        "durata assente o non numerica: {function}"
    );
    assert!(
        entry["arguments"]
            .as_array()
            .is_some_and(|arguments| !arguments.is_empty()),
        "argomenti di ingresso assenti: {function}"
    );
    (entry, exit)
}

fn temporary_directory() -> PathBuf {
    let sequence = TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!("sanikey-ng-aifa-{}-{sequence}", std::process::id()))
}

#[test]
fn aifa_client_constructors_have_nested_trace_spans_and_representable_results() {
    let directory = temporary_directory();
    let (session, log_path) = Session::create_in_directory_with_level(
        &directory.join("logs"),
        Component::Core,
        Level::Trace,
    )
    .expect("sessione sintetica");

    session.scoped(|| {
        let client = AifaClient::new();
        let _default_client = AifaClient::default();
        let medication = Medication {
            id: "synthetic".into(),
            name: "synthetic".into(),
            active_ingredient: None,
            form: None,
            strength_per_unit: None,
        };
        let (result, classified) = client.lookup_candidate_search_classified(&medication, Some(""));
        assert!(result.is_some());
        assert!(!classified);
    });
    drop(session);

    let log = fs::read_to_string(log_path).expect("log sintetico");
    let records = log
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).expect("record JSON valido"))
        .collect::<Vec<_>>();
    assert!(
        records
            .iter()
            .any(|record| record["type"] == "function_exit"
                && record["function"] == "aifa::AifaClient::lookup_candidate_search_classified")
    );
    for function in ["aifa::AifaClient::new", "aifa::AifaClient::default"] {
        let entry = records
            .iter()
            .find(|record| record["type"] == "function_enter" && record["function"] == function)
            .unwrap_or_else(|| panic!("span di ingresso assente: {function}"));
        let exit = records
            .iter()
            .find(|record| record["type"] == "function_exit" && record["function"] == function)
            .unwrap_or_else(|| panic!("span di uscita assente: {function}"));
        assert_eq!(exit["outcome"], "returned");
        assert!(exit["duration_ms"].is_number());
        if function.ends_with("default") {
            assert!(entry["arguments"].to_string().contains("AifaClient::new"));
        } else {
            assert!(entry["arguments"].to_string().contains("api.aifa.gov.it"));
        }
        assert!(exit["result"].to_string().contains("api.aifa.gov.it"));
        assert!(exit["result"].to_string().contains("unsupported_type"));
    }
    let default_entry = records
        .iter()
        .find(|record| {
            record["type"] == "function_enter" && record["function"] == "aifa::AifaClient::default"
        })
        .expect("span Default::default");
    let nested_new = records
        .iter()
        .find(|record| {
            record["type"] == "function_enter"
                && record["function"] == "aifa::AifaClient::new"
                && record["parent_call_id"] == default_entry["call_id"]
        })
        .expect("span new annidata nel Default");
    assert!(nested_new["call_id"].is_number());

    fs::remove_dir_all(directory).expect("pulizia log sintetico");
}

#[test]
fn confirms_candidate_with_fingerprint_of_curated_fields() {
    let medication = Medication {
        id: "drug-a".into(),
        name: " Drug A ".into(),
        active_ingredient: Some("INGREDIENT".into()),
        form: Some("Tablet".into()),
        strength_per_unit: Some("10 MG".into()),
    };
    let candidate = AifaCandidate {
        title: "Drug A".into(),
        codice_sis: "123".into(),
        aic6: "456".into(),
        active_ingredients: vec![],
        form: None,
        strength: None,
        company: None,
        atc: vec![],
        routes: vec![],
        packages: vec![],
        mismatches: vec![],
    };

    let reference = confirm_aifa_candidate(&medication, &candidate);

    assert_eq!(reference.medication_id, "drug-a");
    assert_eq!(reference.codice_sis, "123");
    assert_eq!(reference.aic6, "456");
    assert_eq!(
        reference.source_fingerprint.as_deref(),
        Some(medication_fingerprint(&medication).as_str())
    );
    assert_eq!(
        medication_fingerprint(&medication),
        "39a6161baf9e86a465f57f42a39875969c7da6363b41e05e9c8788c9dbf6e72d"
    );
    assert_eq!(
        medication_fingerprint(&medication),
        medication_fingerprint(&Medication {
            id: "different-id".into(),
            name: "drug a".into(),
            active_ingredient: Some("ingredient".into()),
            form: Some("tablet".into()),
            strength_per_unit: Some("10 mg".into()),
        })
    );
}

#[test]
fn persists_sorted_references_and_exclusions_as_loadable_toml() {
    let directory = temporary_directory();
    fs::create_dir_all(&directory).expect("creazione directory metadati");
    fs::write(
        directory.join("medications.toml"),
        "[[medication]]\nid = \"a-drug\"\nname = \"A Drug\"\n\n[[medication]]\nid = \"z-drug\"\nname = \"Z Drug\"\n\n[[medication]]\nid = \"supplement\"\nname = \"Supplement\"\n",
    )
    .expect("scrittura farmaci sintetici");
    let references = [
        crate::metadata::MedicationLeaflet {
            medication_id: "z-drug".into(),
            codice_sis: "123".into(),
            aic6: "456".into(),
            downloaded_at: None,
            source_fingerprint: Some("fingerprint-\"quoted\"".into()),
        },
        crate::metadata::MedicationLeaflet {
            medication_id: "a-drug".into(),
            codice_sis: "789".into(),
            aic6: "012".into(),
            downloaded_at: None,
            source_fingerprint: None,
        },
    ];
    let exclusions = [MedicationLeafletExclusion {
        medication_id: "supplement".into(),
        reason: "non_aifa".into(),
    }];

    let target = write_confirmed_references(&directory, &references, &exclusions)
        .expect("scrittura TOML riferimenti");
    let content = fs::read_to_string(&target).expect("lettura TOML riferimenti");
    let parsed: toml::Value = toml::from_str(&content).expect("TOML valido");
    let first_id = parsed["leaflet"][0]["medication_id"].as_str();

    assert_eq!(first_id, Some("a-drug"));
    assert_eq!(
        parsed["leaflet"][1]["source_fingerprint"].as_str(),
        Some("fingerprint-\"quoted\"")
    );
    assert_eq!(
        parsed["unavailable"][0]["medication_id"].as_str(),
        Some("supplement")
    );
    let loaded =
        crate::metadata::load_curated_metadata(&directory).expect("rilettura metadati persistiti");
    assert_eq!(
        loaded.medication_leaflets,
        [references[1].clone(), references[0].clone()]
    );
    assert_eq!(loaded.medication_leaflet_exclusions, exclusions);
    fs::remove_dir_all(directory).expect("pulizia directory sintetica");
}

#[test]
fn update_reference_merges_unrelated_changes_made_after_review_loaded() {
    let directory = temporary_directory();
    fs::create_dir_all(&directory).expect("creazione directory metadati");
    fs::write(
        directory.join("medications.toml"),
        "[[medication]]\nid='drug-a'\nname='Drug A'\n\n[[medication]]\nid='drug-b'\nname='Drug B'\n",
    )
    .expect("farmaci sintetici");
    let snapshot = crate::metadata::load_curated_metadata(&directory).expect("snapshot iniziale");
    let expected_fingerprint = medication_fingerprint(&snapshot.medications[0]);

    write_confirmed_references(
        &directory,
        &[MedicationLeaflet {
            medication_id: "drug-b".into(),
            codice_sis: "222".into(),
            aic6: "333".into(),
            downloaded_at: None,
            source_fingerprint: None,
        }],
        &[],
    )
    .expect("modifica concorrente all'altro farmaco");

    let updated = update_confirmed_reference(
        &directory,
        "drug-a",
        &expected_fingerprint,
        None,
        None,
        ConfirmedReferenceDecision::Confirm(MedicationLeaflet {
            medication_id: "drug-a".into(),
            codice_sis: "111".into(),
            aic6: "444".into(),
            downloaded_at: None,
            source_fingerprint: Some(expected_fingerprint.clone()),
        }),
    )
    .expect("merge della modifica recente");

    assert_eq!(updated.medication_leaflets.len(), 2);
    assert!(
        updated
            .medication_leaflets
            .iter()
            .any(|item| { item.medication_id == "drug-a" && item.aic6 == "444" })
    );
    assert!(
        updated
            .medication_leaflets
            .iter()
            .any(|item| { item.medication_id == "drug-b" && item.aic6 == "333" })
    );
    fs::remove_dir_all(directory).expect("pulizia directory sintetica");
}

#[test]
fn update_reference_rejects_a_concurrent_change_to_the_same_medication() {
    let directory = temporary_directory();
    fs::create_dir_all(&directory).expect("creazione directory metadati");
    fs::write(
        directory.join("medications.toml"),
        "[[medication]]\nid='drug-a'\nname='Drug A'\n",
    )
    .expect("farmaco sintetico");
    let snapshot = crate::metadata::load_curated_metadata(&directory).expect("snapshot iniziale");
    let expected_fingerprint = medication_fingerprint(&snapshot.medications[0]);
    write_confirmed_references(
        &directory,
        &[MedicationLeaflet {
            medication_id: "drug-a".into(),
            codice_sis: "222".into(),
            aic6: "333".into(),
            downloaded_at: None,
            source_fingerprint: None,
        }],
        &[],
    )
    .expect("modifica concorrente dello stesso farmaco");

    let result = update_confirmed_reference(
        &directory,
        "drug-a",
        &expected_fingerprint,
        None,
        None,
        ConfirmedReferenceDecision::Confirm(MedicationLeaflet {
            medication_id: "drug-a".into(),
            codice_sis: "111".into(),
            aic6: "444".into(),
            downloaded_at: None,
            source_fingerprint: Some(expected_fingerprint.clone()),
        }),
    );
    let latest = crate::metadata::load_curated_metadata(&directory).expect("snapshot corrente");

    assert!(result.is_err());
    assert_eq!(latest.medication_leaflets.len(), 1);
    assert_eq!(latest.medication_leaflets[0].aic6, "333");
    fs::remove_dir_all(directory).expect("pulizia directory sintetica");
}

#[test]
fn simultaneous_reference_updates_are_serialized_and_one_stale_writer_conflicts() {
    let directory = temporary_directory();
    fs::create_dir_all(&directory).expect("creazione directory metadati");
    fs::write(
        directory.join("medications.toml"),
        "[[medication]]\nid='drug-a'\nname='Drug A'\n",
    )
    .expect("farmaco sintetico");
    let snapshot = crate::metadata::load_curated_metadata(&directory).expect("snapshot iniziale");
    let expected_fingerprint = medication_fingerprint(&snapshot.medications[0]);
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(3));
    let writers = ["111", "222"].map(|aic6| {
        let directory = directory.clone();
        let expected_fingerprint = expected_fingerprint.clone();
        let barrier = barrier.clone();
        std::thread::spawn(move || {
            barrier.wait();
            update_confirmed_reference(
                &directory,
                "drug-a",
                &expected_fingerprint,
                None,
                None,
                ConfirmedReferenceDecision::Confirm(MedicationLeaflet {
                    medication_id: "drug-a".into(),
                    codice_sis: "123".into(),
                    aic6: aic6.into(),
                    downloaded_at: None,
                    source_fingerprint: Some(expected_fingerprint.clone()),
                }),
            )
        })
    });
    barrier.wait();
    let results = writers.map(|writer| writer.join().expect("writer terminato"));
    let latest = crate::metadata::load_curated_metadata(&directory).expect("snapshot finale");

    assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
    assert_eq!(results.iter().filter(|result| result.is_err()).count(), 1);
    assert_eq!(latest.medication_leaflets.len(), 1);
    assert!(["111", "222"].contains(&latest.medication_leaflets[0].aic6.as_str()));
    fs::remove_dir_all(directory).expect("pulizia directory sintetica");
}

#[cfg(unix)]
#[test]
fn atomic_reference_write_preserves_permissions_and_keeps_temp_and_lock_private() {
    use std::io::Write as _;
    use std::os::unix::fs::PermissionsExt;

    let directory = temporary_directory();
    fs::create_dir_all(&directory).expect("creazione directory metadati");
    let target = directory.join("medication_leaflets.toml");
    fs::write(
        &target,
        "[[unavailable]]\nmedication_id='old'\nreason='non_aifa'\n",
    )
    .expect("TOML privato iniziale");
    fs::set_permissions(&target, fs::Permissions::from_mode(0o640))
        .expect("permessi TOML iniziali");

    let (temporary, mut file) = create_private_temp_file(&directory).expect("temporaneo privato");
    assert_eq!(
        file.metadata()
            .expect("metadati temporaneo")
            .permissions()
            .mode()
            & 0o7777,
        0o600
    );
    file.write_all(b"fixture privata sintetica")
        .expect("scrittura temporanea");
    assert_eq!(
        file.metadata()
            .expect("metadati temporaneo dopo scrittura")
            .permissions()
            .mode()
            & 0o7777,
        0o600
    );
    drop(file);
    fs::remove_file(temporary).expect("rimozione temporaneo di prova");

    write_confirmed_references(
        &directory,
        &[MedicationLeaflet {
            medication_id: "drug-a".into(),
            codice_sis: "123".into(),
            aic6: "456".into(),
            downloaded_at: None,
            source_fingerprint: None,
        }],
        &[],
    )
    .expect("sostituzione atomica");

    assert_eq!(
        fs::metadata(&target)
            .expect("TOML sostituito")
            .permissions()
            .mode()
            & 0o7777,
        0o640
    );
    let lock = directory.join(".medication_leaflets.lock");
    assert_eq!(fs::metadata(&lock).expect("lock").len(), 0);
    assert_eq!(
        fs::metadata(&lock)
            .expect("permessi lock")
            .permissions()
            .mode()
            & 0o7777,
        0o600
    );
    assert!(
        fs::read_dir(&directory)
            .expect("directory metadati")
            .all(|entry| !entry
                .expect("voce directory")
                .file_name()
                .to_string_lossy()
                .ends_with(".tmp"))
    );
    fs::remove_dir_all(directory).expect("pulizia directory sintetica");
}

#[test]
fn downloads_and_records_only_a_complete_pdf_pair() {
    let directory = temporary_directory();
    let reference = MedicationLeaflet {
        medication_id: "drug-a".into(),
        codice_sis: "123".into(),
        aic6: "456".into(),
        downloaded_at: None,
        source_fingerprint: None,
    };
    let mut requests = Vec::new();
    let result = download_confirmed_leaflets_with(&directory, &[reference], |url| {
        requests.push(url.to_owned());
        Ok(b"%PDF-synthetic".to_vec())
    })
    .expect("download sintetico");

    assert!(result.failures.is_empty());
    assert_eq!(requests.len(), 2);
    assert!(requests[0].ends_with("/123/farmaci/456/stampati?ts=FI"));
    assert!(requests[1].ends_with("/123/farmaci/456/stampati?ts=RCP"));
    assert_eq!(
        fs::read(directory.join("medication-leaflets/drug-a/foglio-illustrativo.pdf"))
            .expect("FI scaricato"),
        b"%PDF-synthetic"
    );
    assert_eq!(
        fs::read(directory.join("medication-leaflets/drug-a/rcp.pdf")).expect("RCP scaricato"),
        b"%PDF-synthetic"
    );
    assert_eq!(leaflet_download_dates(&directory), result.downloaded_at);
    assert!(result.downloaded_at.contains_key("drug-a"));
    fs::remove_dir_all(directory).expect("pulizia directory sintetica");
}

#[test]
fn failed_document_keeps_the_existing_pair_and_cached_date() {
    let directory = temporary_directory();
    let target = directory.join("medication-leaflets/drug-a");
    fs::create_dir_all(&target).expect("creazione cache sintetica");
    fs::write(target.join("foglio-illustrativo.pdf"), b"%PDF-old-fi").expect("FI precedente");
    fs::write(target.join("rcp.pdf"), b"%PDF-old-rcp").expect("RCP precedente");
    fs::create_dir_all(directory.join("manifests")).expect("creazione manifest sintetico");
    fs::write(
        directory.join("manifests/medication-leaflets.json"),
        r#"{"downloaded_at":{"drug-a":"2026-10-27"}}"#,
    )
    .expect("manifest precedente");
    let reference = MedicationLeaflet {
        medication_id: "drug-a".into(),
        codice_sis: "123".into(),
        aic6: "456".into(),
        downloaded_at: None,
        source_fingerprint: None,
    };
    let result = download_confirmed_leaflets_with(&directory, &[reference], |url| {
        if url.ends_with("ts=RCP") {
            Err("HTTP 503: servizio sospeso".into())
        } else {
            Ok(b"%PDF-new-fi".to_vec())
        }
    })
    .expect("un errore per documento resta nel risultato");

    assert_eq!(result.downloaded_at["drug-a"], "2026-10-27");
    assert_eq!(result.failures.len(), 1);
    assert_eq!(result.failures[0].kind, "RCP");
    assert_eq!(result.failures[0].reason, "HTTP 503: servizio sospeso");
    assert_eq!(
        fs::read(target.join("foglio-illustrativo.pdf")).expect("FI conservato"),
        b"%PDF-old-fi"
    );
    assert_eq!(
        fs::read(target.join("rcp.pdf")).expect("RCP conservato"),
        b"%PDF-old-rcp"
    );
    fs::remove_dir_all(directory).expect("pulizia directory sintetica");
}

#[test]
fn rejects_non_pdf_bodies_and_extracts_aifa_error_details() {
    let reference = MedicationLeaflet {
        medication_id: "drug-a".into(),
        codice_sis: "123".into(),
        aic6: "456".into(),
        downloaded_at: None,
        source_fingerprint: None,
    };
    let (_, failures) = super::fetch_leaflet_documents_with(&reference, |_| {
        Ok(br#"{"message":"non disponibile"}"#.to_vec())
    });

    assert_eq!(failures.len(), 2);
    assert!(
        failures
            .iter()
            .all(|failure| { failure.reason == "risposta AIFA non in formato PDF" })
    );
    assert_eq!(
        super::aifa_error_detail(r#"{"errors":["Stampato RCP/FI non disponibile"]}"#).as_deref(),
        Some("Stampato RCP/FI non disponibile")
    );
}

#[test]
fn finds_cached_leaflet_files_for_offline_use() {
    let directory = temporary_directory();
    let cache = directory.join("medication-leaflets/drug-a");
    fs::create_dir_all(&cache).expect("creazione cache sintetica");
    fs::write(cache.join("foglio-illustrativo.pdf"), b"%PDF-synthetic").expect("FI sintetico");
    fs::write(cache.join("rcp.pdf"), b"%PDF-synthetic").expect("RCP sintetico");

    let documents = local_leaflet_documents(&directory, "drug-a");

    assert_eq!(
        documents.leaflet_pdf,
        Some(cache.join("foglio-illustrativo.pdf"))
    );
    assert_eq!(documents.rcp_pdf, Some(cache.join("rcp.pdf")));
    assert_eq!(
        local_leaflet_documents(&directory, "missing-drug"),
        Default::default()
    );
    fs::remove_dir_all(directory).expect("pulizia directory sintetica");
}

#[test]
fn distinguishes_unavailable_service_from_empty_catalog_response() {
    let medication = Medication {
        id: "drug-a".into(),
        name: "Drug A".into(),
        active_ingredient: None,
        form: None,
        strength_per_unit: None,
    };
    let mut unavailable = SearchFixture::new(vec![None]);

    assert_eq!(
        lookup_candidate_search_with(&medication, None, |path| unavailable.fetch(path)),
        None
    );
    assert_eq!(
        unavailable.requests,
        ["formadosaggio/ricerca?query=Drug+A&page=0"]
    );

    let mut empty = SearchFixture::new(vec![success(response_with_candidates(vec![]))]);
    assert_eq!(
        lookup_candidate_search_with(&medication, None, |path| empty.fetch(path)),
        Some(AifaCandidateSearch::default())
    );
}

#[test]
fn keeps_only_exact_dosage_and_form_as_confirmed_candidates() {
    let medication = Medication {
        id: "atenololo".into(),
        name: "Atenololo 100 mg".into(),
        active_ingredient: Some("Atenololo".into()),
        form: Some("compresse".into()),
        strength_per_unit: Some("100 mg".into()),
    };
    let payload = response_with_candidates(vec![
        candidate(
            "ATENOLOLO",
            &["ATENOLOLO"],
            "Compressa",
            "25 mg COMPRESSA",
            123,
        ),
        candidate(
            "ATENOLOLO",
            &["ATENOLOLO"],
            "Compressa",
            "50 mg COMPRESSA",
            124,
        ),
        candidate(
            "ATENOLOLO",
            &["ATENOLOLO"],
            "Compressa",
            "100 mg COMPRESSA",
            125,
        ),
    ]);
    let mut fixture = SearchFixture::new(vec![success(payload.clone()), success(payload)]);

    let search = lookup_candidate_search_with(&medication, None, |path| fixture.fetch(path))
        .expect("risposta AIFA disponibile");

    assert_eq!(search.exact.len(), 1);
    assert_eq!(
        search.exact[0].strength.as_deref(),
        Some("100 mg COMPRESSA")
    );
    assert_eq!(search.review.len(), 2);
    assert_eq!(search.all_candidates.len(), 3);
    assert_eq!(
        fixture.requests,
        [
            "formadosaggio/ricerca?query=Atenololo+100+mg&page=0",
            "formadosaggio/ricerca?query=Atenololo&page=0"
        ]
    );
}

#[test]
fn classifies_plausible_formulation_mismatches_for_operator_review() {
    let medication = Medication {
        id: "zanedip".into(),
        name: "Zanedip 10 mg".into(),
        active_ingredient: Some("Lercanidipina".into()),
        form: Some("compresse".into()),
        strength_per_unit: Some("10 mg".into()),
    };
    let payload = response_with_candidates(vec![
        candidate(
            "ZANEDIP",
            &["LERCANIDIPINA"],
            "Gocce orali",
            "10 mg GOCCE",
            123,
        ),
        candidate("DIS 10", &["SODIO"], "Gocce orali", "10 mg GOCCE", 124),
    ]);
    let mut fixture = SearchFixture::new(vec![success(payload)]);

    let search =
        lookup_candidate_search_with(&medication, Some("Zanedip"), |path| fixture.fetch(path))
            .expect("risposta AIFA disponibile");

    assert!(search.exact.is_empty());
    assert_eq!(search.review.len(), 1);
    assert_eq!(search.review[0].title, "ZANEDIP");
    assert_eq!(search.review[0].mismatches, ["forma"]);
    assert_eq!(search.all_candidates.len(), 2);
    assert_eq!(
        fixture.requests,
        ["formadosaggio/ricerca?query=Zanedip&page=0"]
    );
}

#[test]
fn active_ingredient_components_and_dose_signatures_match_combinations() {
    let medication = Medication {
        id: "codamol".into(),
        name: "Codamol 500 mg + 30 mg".into(),
        active_ingredient: Some("Paracetamolo e codeina".into()),
        form: Some("compresse".into()),
        strength_per_unit: Some("500/30".into()),
    };
    let payload = response_with_candidates(vec![candidate(
        "CODAMOL",
        &["PARACETAMOLO", "CODEINA FOSFATO"],
        "Compressa",
        "500 mg + 30 mg COMPRESSA",
        123,
    )]);
    let mut fixture = SearchFixture::new(vec![success(payload.clone()), success(payload)]);

    let search = lookup_candidate_search_with(&medication, None, |path| fixture.fetch(path))
        .expect("risposta AIFA disponibile");

    assert_eq!(search.exact.len(), 1);
    assert_eq!(
        search.exact[0].active_ingredients,
        ["PARACETAMOLO", "CODEINA FOSFATO"]
    );
    assert_eq!(search.all_candidates.len(), 1);
    assert_eq!(fixture.requests.len(), 2);
}

#[test]
fn malformed_catalog_items_are_discarded_without_failing_successful_search() {
    let medication = Medication {
        id: "drug-a".into(),
        name: "Drug A".into(),
        active_ingredient: None,
        form: None,
        strength_per_unit: None,
    };
    let payload = json!({"data": {"content": [null, {}, {
        "medicinale": {
            "codiceSis": 123,
            "aic6": "456",
            "denominazioneMedicinale": "Drug A"
        }
    }]}});
    let mut fixture = SearchFixture::new(vec![success(payload)]);

    let search = lookup_candidate_search_with(&medication, None, |path| fixture.fetch(path))
        .expect("risposta AIFA disponibile");

    assert_eq!(search.all_candidates.len(), 1);
    assert_eq!(search.exact.len(), 1);
    assert_eq!(search.exact[0].codice_sis, "123");
}

#[test]
fn automatic_search_deduplicates_normalized_query_terms() {
    let medication = Medication {
        id: "drug-a".into(),
        name: "  Drug A  ".into(),
        active_ingredient: Some("drug a".into()),
        form: None,
        strength_per_unit: None,
    };
    let payload = response_with_candidates(vec![candidate("Drug A", &["Drug A"], "", "", 123)]);
    let mut fixture = SearchFixture::new(vec![success(payload)]);

    let search = lookup_candidate_search_with(&medication, None, |path| fixture.fetch(path))
        .expect("risposta AIFA disponibile");

    assert_eq!(search.exact.len(), 1);
    assert_eq!(
        fixture.requests,
        ["formadosaggio/ricerca?query=Drug+A&page=0"]
    );
}

#[test]
fn synthetic_trace_captures_aifa_request_and_pdf_bytes_but_events_stay_terse() {
    let directory = temporary_directory();
    let medication = Medication {
        id: "drug-private".into(),
        name: "Synthetic Private Medication".into(),
        active_ingredient: Some("Synthetic Ingredient".into()),
        form: Some("capsule".into()),
        strength_per_unit: Some("12 mg".into()),
    };
    let reference = MedicationLeaflet {
        medication_id: medication.id.clone(),
        codice_sis: "123".into(),
        aic6: "456".into(),
        downloaded_at: None,
        source_fingerprint: None,
    };

    for (suffix, level) in [("events", Level::Events), ("trace", Level::Trace)] {
        let build_root = directory.join(format!("build-{suffix}"));
        let logs = directory.join(format!("logs-{suffix}"));
        let (session, log_path) =
            Session::create_in_directory_with_level(&logs, Component::Core, level)
                .expect("sessione sintetica");
        session.scoped(|| {
            let mut payload = response_with_candidates(vec![candidate(
                "Synthetic Private Medication",
                &["Synthetic Ingredient"],
                "capsule",
                "12 mg capsule",
                123,
            )]);
            payload["nested"] = serde_json::json!({
                "api_key": "synthetic-aifa-payload-secret",
                "ordinary": "private payload detail",
            });
            let mut fixture = SearchFixture::new(vec![success(payload)]);
            let search =
                lookup_candidate_search_with(&medication, None, |path| fixture.fetch(path));
            assert!(search.is_some());

            let downloaded = download_confirmed_leaflets_with(
                &build_root,
                std::slice::from_ref(&reference),
                |_| Ok(b"%PDF-synthetic-private-leaflet".to_vec()),
            )
            .expect("download sintetico");
            assert!(downloaded.failures.is_empty());

            let failed_reference = MedicationLeaflet {
                medication_id: "drug-failure".into(),
                ..reference.clone()
            };
            let failed = super::fetch_leaflet_documents_with(&failed_reference, |_| {
                Err("password = synthetic-aifa-secret\nprivate network detail".into())
            });
            assert_eq!(failed.1.len(), 2);
        });
        drop(session);

        let log = fs::read_to_string(log_path).expect("log sintetico");
        if level == Level::Trace {
            assert!(log.contains("aifa::lookup_candidate_search_with"));
            assert!(log.contains("aifa::lookup_queries"));
            assert!(log.contains("aifa::catalog_request"));
            assert!(log.contains("formadosaggio/ricerca?query=Synthetic+Private+Medication"));
            assert!(log.contains("Synthetic Private Medication"));
            assert!(log.contains("private payload detail"));
            assert!(log.contains("secret_omitted"));
            assert!(!log.contains("synthetic-aifa-payload-secret"));
            assert!(!log.contains("aifa::AifaClient::get_pdf"));
            assert!(log.contains("aifa::fetch_leaflet_documents_with"));
            assert!(log.contains("downloaded"));
            let encoded_pdf = serde_json::to_string(b"%PDF-synthetic-private-leaflet")
                .expect("byte fixture JSON");
            assert!(log.contains(&encoded_pdf));
            assert!(log.contains("private network detail"));
            assert!(log.contains("secret_omitted"));
            assert!(!log.contains("synthetic-aifa-secret"));
            assert!(log.contains("external_component"));
        } else {
            assert!(!log.contains("function_enter"));
            assert!(!log.contains("Synthetic Private Medication"));
            assert!(!log.contains("synthetic-private-leaflet"));
            assert!(!log.contains("private network detail"));
            assert!(!log.contains("synthetic-aifa-secret"));
        }
    }
    fs::remove_dir_all(directory).expect("pulizia directory sintetica");
}

#[test]
fn aifa_json_request_errors_trace_http_status_and_transport_without_network() {
    let directory = temporary_directory();
    let (session, log_path) = Session::create_in_directory_with_level(
        &directory.join("logs"),
        Component::Core,
        Level::Trace,
    )
    .expect("sessione Trace sintetica");

    let (http_error, transport_error, secret_error) = session.scoped(|| {
        let response = ureq::Response::new(
            503,
            "Service Unavailable",
            r#"{"errors":["synthetic service unavailable"]}"#,
        )
        .expect("risposta HTTP sintetica");
        let http_error =
            trace_aifa_json_request_error(ureq::Error::Status(503, response), true, false)
                .expect("errore HTTP tracciato")
                .to_json();

        let transport_error = trace_aifa_json_request_error(
            ureq::Error::from(std::io::Error::other("synthetic transport failure")),
            true,
            false,
        )
        .expect("errore di trasporto tracciato")
        .to_json();

        let secret_response = ureq::Response::new(503, "Service Unavailable", "synthetic")
            .expect("risposta HTTP sintetica");
        let secret_error =
            trace_aifa_json_request_error(ureq::Error::Status(503, secret_response), true, true);

        (http_error, transport_error, secret_error)
    });
    drop(session);

    assert!(http_error.to_string().contains("HTTP 503"));
    assert!(
        http_error
            .to_string()
            .contains("synthetic service unavailable")
    );
    assert!(transport_error.to_string().contains("errore di rete"));
    assert!(
        transport_error
            .to_string()
            .contains("synthetic transport failure")
    );
    assert!(secret_error.is_none());

    let log = fs::read_to_string(log_path).expect("log sintetico");
    assert!(log.contains("aifa::aifa_request_error"));
    assert!(log.contains("HTTP 503"));
    assert!(log.contains("synthetic service unavailable"));
    assert!(log.contains("synthetic transport failure"));
    fs::remove_dir_all(directory).expect("pulizia directory sintetica");

    let event_directory = temporary_directory();
    let (events, event_path) = Session::create_in_directory_with_level(
        &event_directory.join("logs"),
        Component::Core,
        Level::Events,
    )
    .expect("sessione Events sintetica");
    let event_response = ureq::Response::new(503, "Service Unavailable", "synthetic")
        .expect("risposta HTTP sintetica");
    let event_error = events.scoped(|| {
        trace_aifa_json_request_error(ureq::Error::Status(503, event_response), false, false)
    });
    assert!(event_error.is_none());
    drop(events);
    let events_log = fs::read_to_string(event_path).expect("log Events sintetico");
    assert!(!events_log.contains("function_enter"));
    assert!(!events_log.contains("HTTP 503"));
    fs::remove_dir_all(event_directory).expect("pulizia directory Events sintetica");
}

#[test]
fn lookup_queries_trace_complete_inputs_and_result_or_secret_omission() {
    let directory = temporary_directory();
    let (session, log_path) = Session::create_in_directory_with_level(
        &directory.join("logs"),
        Component::Core,
        Level::Trace,
    )
    .expect("sessione Trace sintetica");
    let safe_medication = Medication {
        id: "drug-safe".into(),
        name: "Farmaco di controllo".into(),
        active_ingredient: Some("Principio di controllo".into()),
        form: Some("capsula".into()),
        strength_per_unit: Some("10 mg".into()),
    };
    let sensitive_medication = Medication {
        id: "drug-query-secret".into(),
        name: "Farmaco query sintetica".into(),
        active_ingredient: Some("Principio query sintetico".into()),
        form: Some("compressa".into()),
        strength_per_unit: Some("25 mg".into()),
    };
    let secret_query = "password=synthetic-aifa-lookup-query-secret";

    session.scoped(|| {
        assert_eq!(
            super::lookup_queries(&safe_medication, None),
            ["Farmaco di controllo", "Principio di controllo"]
        );
        assert_eq!(
            super::lookup_queries(&sensitive_medication, Some(secret_query)),
            [secret_query]
        );
    });
    drop(session);

    let log = fs::read_to_string(log_path).expect("log sintetico");
    let records = log
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).expect("record JSON valido"))
        .collect::<Vec<_>>();
    let entries = records
        .iter()
        .filter(|record| {
            record["type"] == "function_enter" && record["function"] == "aifa::lookup_queries"
        })
        .collect::<Vec<_>>();
    assert_eq!(entries.len(), 2, "attesi i due ingressi sintetici");

    let verify_exit = |entry: &Value, expected: Value| {
        let exit = records
            .iter()
            .find(|record| {
                record["type"] == "function_exit"
                    && record["function"] == "aifa::lookup_queries"
                    && record["call_id"] == entry["call_id"]
            })
            .expect("uscita abbinata tramite call_id");
        assert!(entry["call_id"].is_u64());
        assert_eq!(exit["call_id"], entry["call_id"]);
        assert_eq!(exit["outcome"], "returned");
        assert!(exit["duration_ms"].is_u64());
        assert_eq!(exit["result"], expected);
    };

    let safe_entry = entries
        .iter()
        .find(|entry| entry["arguments"][0]["content"]["value"]["id"] == "drug-safe")
        .expect("ingresso farmaco non sensibile");
    assert_eq!(
        safe_entry["arguments"],
        json!([
            {
                "name": "medication",
                "content": {
                    "kind": "data",
                    "value": {
                        "id": "drug-safe",
                        "name": "Farmaco di controllo",
                        "active_ingredient": "Principio di controllo",
                        "form": "capsula",
                        "strength_per_unit": "10 mg"
                    }
                }
            },
            {
                "name": "query",
                "content": {"kind": "data", "value": null}
            }
        ])
    );
    verify_exit(
        safe_entry,
        json!({
            "kind": "data",
            "value": {
                "result": {
                    "kind": "data",
                    "value": ["Farmaco di controllo", "Principio di controllo"]
                }
            }
        }),
    );

    let sensitive_entry = entries
        .iter()
        .find(|entry| entry["arguments"][0]["content"]["value"]["id"] == "drug-query-secret")
        .expect("ingresso farmaco con query sensibile");
    assert_eq!(
        sensitive_entry["arguments"],
        json!([
            {
                "name": "medication",
                "content": {
                    "kind": "data",
                    "value": {
                        "id": "drug-query-secret",
                        "name": "Farmaco query sintetica",
                        "active_ingredient": "Principio query sintetico",
                        "form": "compressa",
                        "strength_per_unit": "25 mg"
                    }
                }
            },
            {
                "name": "query",
                "content": {
                    "kind": "data",
                    "value": {
                        "format": "text_redacted_known_secret_assignments",
                        "value": "password=[secret_omitted]"
                    }
                }
            }
        ])
    );
    verify_exit(
        sensitive_entry,
        json!({
            "kind": "data",
            "value": {"result": {"kind": "secret_omitted"}}
        }),
    );
    assert!(!log.contains(secret_query));
    fs::remove_dir_all(directory).expect("pulizia log sintetico");
}

#[test]
fn derived_search_results_and_pdf_flows_keep_secret_provenance() {
    let directory = temporary_directory();
    let logs = directory.join("logs");
    let (session, log_path) =
        Session::create_in_directory_with_level(&logs, Component::Core, Level::Trace)
            .expect("sessione sintetica");
    let medication = Medication {
        id: "drug-synthetic".into(),
        name: "Ordinary synthetic medication".into(),
        active_ingredient: Some("Other synthetic ingredient".into()),
        form: Some("tablet".into()),
        strength_per_unit: Some("12 mg".into()),
    };
    let reference = MedicationLeaflet {
        medication_id: "drug-synthetic".into(),
        codice_sis: "api_key=synthetic-derived-pdf-secret".into(),
        aic6: "123456".into(),
        downloaded_at: None,
        source_fingerprint: None,
    };

    session.scoped(|| {
        let mut payload = response_with_candidates(vec![candidate(
            "Ordinary candidate title before redaction",
            &["Synthetic ingredient"],
            "capsule",
            "10 mg",
            123,
        )]);
        let derived_secret = "synthetic-derived-search-secret";
        payload["ordinary_note"] = json!("private payload note remains recordable");
        payload["response_metadata"] = json!({
            "api_key": derived_secret
        });
        payload["data"]["content"][0]["medicinale"]["denominazioneMedicinale"] =
            json!(derived_secret);
        let (search, payload_has_secret) =
            super::lookup_candidate_search_with_classification(&medication, None, |_| {
                Some(payload.clone())
            });
        assert!(search.is_some());
        assert!(payload_has_secret);
        assert_eq!(
            search.as_ref().unwrap().all_candidates[0].title,
            derived_secret
        );
        assert!(search.as_ref().unwrap().review.is_empty());

        let mut requested = Vec::new();
        let (downloaded, failures) = super::fetch_leaflet_documents_with(&reference, |url| {
            requested.push(url.to_owned());
            Ok(b"%PDF-synthetic-derived-content".to_vec())
        });
        assert_eq!(downloaded.len(), 2);
        assert!(failures.is_empty());
        assert!(
            requested
                .iter()
                .all(|url| url.contains("synthetic-derived-pdf-secret"))
        );
        super::write_confirmed_references(
            &directory.join("secret-metadata"),
            std::slice::from_ref(&reference),
            &[],
        )
        .expect("scrittura riferimenti sintetici con fonte secret-like");
    });
    drop(session);

    let log = fs::read_to_string(log_path).expect("log sintetico");
    let records = log
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).expect("record JSON valido"))
        .collect::<Vec<_>>();
    for function in [
        "aifa::lookup_candidate_search_with",
        "aifa::lookup_candidate_search_with_impl",
        "aifa::candidates_from_payload",
        "aifa::candidates_from_payload_impl",
        "aifa::ordered_candidates",
        "aifa::candidate_mismatches",
        "aifa::active_ingredients_match",
        "aifa::active_ingredient_components",
        "aifa::form_stems",
        "aifa::dose_signatures",
        "aifa::candidate_is_plausible",
        "aifa::commercial_names_match",
        "aifa::fetch_leaflet_documents_with",
        "aifa::fetch_leaflet_documents_with_impl",
        "aifa::leaflet_document_url",
        "aifa::write_confirmed_references_impl",
        "aifa::write_confirmed_references_unlocked_impl",
        "aifa::write_file_atomically_impl",
    ] {
        let (entry, exit) = assert_trace_span(&records, function);
        if !matches!(
            function,
            "aifa::lookup_candidate_search_with" | "aifa::lookup_candidate_search_with_impl"
        ) {
            assert!(
                entry["arguments"].to_string().contains("secret_omitted"),
                "input derivato non omesso: {function}"
            );
        }
        assert!(
            exit["result"].to_string().contains("secret_omitted"),
            "risultato derivato non omesso: {function}"
        );
    }
    let search_entry = records
        .iter()
        .find(|record| {
            record["type"] == "function_enter"
                && record["function"] == "aifa::lookup_candidate_search_with"
        })
        .expect("argomenti ricerca AIFA");
    assert!(
        search_entry["arguments"]
            .to_string()
            .contains("Ordinary synthetic medication")
    );
    let download_entry = records
        .iter()
        .find(|record| {
            record["type"] == "function_enter"
                && record["function"] == "aifa::fetch_leaflet_documents_with"
        })
        .expect("argomenti download foglietto");
    assert!(
        download_entry["arguments"]
            .to_string()
            .contains("secret_omitted")
    );
    assert!(log.contains("private payload note remains recordable"));
    assert!(!log.contains("Ordinary candidate title before redaction"));
    assert!(!log.contains("synthetic-derived-search-secret"));
    assert!(!log.contains("synthetic-derived-pdf-secret"));
    fs::remove_dir_all(directory).expect("pulizia log sintetico");
}

#[test]
fn aifa_operational_inner_boundaries_have_independent_spans() {
    let directory = temporary_directory();
    let (session, log_path) = Session::create_in_directory_with_level(
        &directory.join("logs"),
        Component::Core,
        Level::Trace,
    )
    .expect("sessione sintetica");
    let metadata_directory = directory.join("metadata");
    fs::create_dir_all(&metadata_directory).expect("directory metadati sintetici");
    fs::write(
        metadata_directory.join("medications.toml"),
        "[[medication]]\nid = 'drug-a'\nname = 'Drug A'\n",
    )
    .expect("farmaco sintetico");
    let metadata = crate::metadata::load_curated_metadata(&metadata_directory)
        .expect("metadati sintetici validi");
    let medication = metadata.medications.first().expect("farmaco presente");
    let fingerprint = super::medication_fingerprint(medication);
    let reference = MedicationLeaflet {
        medication_id: "drug-a".into(),
        codice_sis: "123".into(),
        aic6: "456".into(),
        downloaded_at: None,
        source_fingerprint: Some(fingerprint.clone()),
    };
    let exclusion = MedicationLeafletExclusion {
        medication_id: "drug-b".into(),
        reason: "non_aifa".into(),
    };

    session.scoped(|| {
        super::write_confirmed_references(
            &directory.join("reference-write"),
            std::slice::from_ref(&reference),
            std::slice::from_ref(&exclusion),
        )
        .expect("scrittura riferimenti sintetici");
        super::update_confirmed_reference(
            &metadata_directory,
            "drug-a",
            &fingerprint,
            None,
            None,
            ConfirmedReferenceDecision::Confirm(reference.clone()),
        )
        .expect("aggiornamento riferimento sintetico");
        let _ = super::local_leaflet_documents(&directory, "drug-a");
        let result = super::download_confirmed_leaflets_with(
            &directory,
            std::slice::from_ref(&reference),
            |_| Ok(b"%PDF-synthetic-inner-span".to_vec()),
        );
        assert!(result.is_ok());
    });
    drop(session);

    let log = fs::read_to_string(log_path).expect("log sintetico");
    let records = log
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).expect("record JSON valido"))
        .collect::<Vec<_>>();
    for function in [
        "aifa::write_confirmed_references_impl",
        "aifa::update_confirmed_reference_impl",
        "aifa::write_confirmed_references_unlocked_impl",
        "aifa::lock_confirmed_references_impl",
        "aifa::write_file_atomically_impl",
        "aifa::create_private_temp_file_impl",
        "aifa::local_leaflet_documents_impl",
        "aifa::download_confirmed_leaflets_with_impl",
        "aifa::fetch_leaflet_documents_with_impl",
        "aifa::write_temporary_impl",
        "aifa::write_download_dates_impl",
    ] {
        let (entry, exit) = assert_trace_span(&records, function);
        assert!(entry["arguments"].is_array());
        assert!(!exit["result"].is_null(), "risultato assente: {function}");
    }
    let write_entry = records
        .iter()
        .find(|record| {
            record["type"] == "function_enter"
                && record["function"] == "aifa::write_confirmed_references_impl"
        })
        .expect("ingresso persistenza riferimenti");
    let write_exit = records
        .iter()
        .find(|record| {
            record["type"] == "function_exit"
                && record["function"] == "aifa::write_confirmed_references_impl"
                && record["call_id"] == write_entry["call_id"]
        })
        .expect("uscita persistenza riferimenti");
    assert!(write_entry["arguments"].to_string().contains("drug-a"));
    assert!(
        write_exit["result"]
            .to_string()
            .contains("medication_leaflets.toml")
    );

    let update_entry = records
        .iter()
        .find(|record| {
            record["type"] == "function_enter"
                && record["function"] == "aifa::update_confirmed_reference_impl"
        })
        .expect("ingresso aggiornamento riferimento");
    let update_exit = records
        .iter()
        .find(|record| {
            record["type"] == "function_exit"
                && record["function"] == "aifa::update_confirmed_reference_impl"
                && record["call_id"] == update_entry["call_id"]
        })
        .expect("uscita aggiornamento riferimento");
    assert!(update_entry["arguments"].to_string().contains("drug-a"));
    assert!(update_exit["result"].to_string().contains("456"));

    let download_exit = records
        .iter()
        .find(|record| {
            record["type"] == "function_exit"
                && record["function"] == "aifa::download_confirmed_leaflets_with_impl"
        })
        .expect("uscita download foglietti");
    assert!(
        download_exit["result"]
            .to_string()
            .contains("downloaded_at")
    );
    fs::remove_dir_all(directory).expect("pulizia dati sintetici");
}

#[test]
fn normalized_secret_is_not_reintroduced_in_trace_output() {
    let directory = temporary_directory();
    let (session, path) = Session::create_in_directory_with_level(
        &directory.join("secret-logs"),
        Component::Core,
        Level::Trace,
    )
    .expect("sessione sintetica");
    let output = session.scoped(|| super::normalized_text("password=syntheticsecretaifa", true));
    assert_eq!(output, "password syntheticsecretaifa");
    drop(session);
    let log = fs::read_to_string(path).expect("log sintetico");
    assert!(log.contains("aifa::normalized_text"));
    assert!(log.contains("secret_omitted"));
    assert!(!log.contains("syntheticsecretaifa"));
}

#[test]
fn aifa_parser_and_matching_helpers_omit_results_derived_from_identified_secrets() {
    let directory = temporary_directory();
    let (session, path) = Session::create_in_directory_with_level(
        &directory.join("helper-secret-logs"),
        Component::Core,
        Level::Trace,
    )
    .expect("sessione Trace sintetica");
    let secret = "password=synthetic-aifa-helper-secret; capsule 12 mg";
    let medication = Medication {
        id: "drug-safe".into(),
        name: "Farmaco sintetico".into(),
        active_ingredient: Some(secret.into()),
        form: Some("capsule".into()),
        strength_per_unit: Some("12 mg".into()),
    };
    let payload = json!({
        "data": {"content": [{
            "principiAttiviIt": [secret],
            "formaFarmaceutica": "capsule",
            "descrizioneFormaDosaggio": "12 mg",
            "medicinale": {
                "codiceSis": "123",
                "aic6": "456",
                "denominazioneMedicinale": "Farmaco sintetico",
                "aziendaTitolare": secret
            }
        }]}
    });
    let aifa_candidate = AifaCandidate {
        title: "Farmaco sintetico".into(),
        codice_sis: "123".into(),
        aic6: "456".into(),
        active_ingredients: vec![secret.into()],
        form: Some("capsule".into()),
        strength: Some("12 mg".into()),
        company: Some(secret.into()),
        atc: Vec::new(),
        routes: Vec::new(),
        packages: Vec::new(),
        mismatches: Vec::new(),
    };

    let search_payload = response_with_candidates(vec![candidate(
        "Farmaco sintetico",
        &["principio sintetico"],
        "capsule",
        "12 mg",
        123,
    )]);
    let mut fixture = SearchFixture::new(vec![success(search_payload)]);
    let build_root = directory.join("helper-build");
    let manifests = build_root.join("manifests");
    fs::create_dir_all(&manifests).expect("directory manifest sintetica");
    fs::write(
        manifests.join("medication-leaflets.json"),
        json!({"downloaded_at":{"drug-safe":secret}}).to_string(),
    )
    .expect("manifest sintetico");
    session.scoped(|| {
        let encoded_secret_path =
            "formadosaggio/ricerca?query=password%3Dsynthetic-aifa-helper-secret%3B+capsule&page=0";
        assert!(super::request_path_contains_identified_secret(encoded_secret_path));
        assert_eq!(
            super::trace_aifa_request_url(
                "https://example.invalid/formadosaggio/ricerca?query=password%3Dsynthetic-aifa-helper-secret",
                encoded_secret_path,
            )
            .to_json()["kind"],
            "secret_omitted"
        );
        assert_eq!(
            super::trace_aifa_request_url(
                "https://example.invalid/formadosaggio/ricerca?query=Farmaco+sicuro",
                "formadosaggio/ricerca?query=Farmaco+sicuro&page=0",
            )
            .to_json()["value"]["value"],
            "https://example.invalid/formadosaggio/ricerca?query=Farmaco+sicuro"
        );
        assert!(!super::active_ingredient_components(secret, false).is_empty());
        assert!(super::form_stems(secret, false).contains("capsul"));
        assert!(
            super::dose_signatures(secret, false).contains(&("12".to_owned(), "mg".to_owned()))
        );
        assert_eq!(
            super::json_string(&Value::String(secret.into()), false).as_deref(),
            Some(secret)
        );
        assert_eq!(
            super::optional_string(Some(&Value::String(secret.into())), false).as_deref(),
            Some(secret)
        );
        assert_eq!(
            super::string_array(Some(&json!([secret])), false),
            [secret.to_owned()]
        );
        assert_eq!(
            super::aifa_error_detail_impl(&format!(r#"{{"errors":["{secret}"]}}"#))
                .as_deref(),
            Some(secret)
        );
        assert_eq!(
            super::aifa_error_detail(&format!(r#"{{"errors":["{secret}"]}}"#)).as_deref(),
            Some(secret)
        );
        assert_eq!(super::medication_fingerprint(&medication).len(), 64);
        assert!(super::confirm_aifa_candidate(&medication, &aifa_candidate)
            .source_fingerprint
            .is_some());
        assert_eq!(
            super::leaflet_download_dates_impl(&build_root).get("drug-safe"),
            Some(&secret.to_owned())
        );
        assert_eq!(super::candidates_from_payload_impl(&payload).len(), 1);
        assert_eq!(super::candidates_from_payload(&payload).len(), 1);
        assert_eq!(
            super::ordered_candidates(vec![aifa_candidate.clone()], false).len(),
            1
        );
        let _ = super::candidate_mismatches(&aifa_candidate, &medication, false);
        let _ = super::active_ingredients_match(&aifa_candidate, &medication, false);
        let _ = super::candidate_is_plausible(&aifa_candidate, &medication, false);
        let _ = super::commercial_names_match(secret, "farmaco sintetico", false);
        assert!(
            super::lookup_candidate_search_with(&medication, Some(secret), |request| fixture
                .fetch(request),)
            .is_some()
        );
    });
    drop(session);

    let log = fs::read_to_string(path).expect("log sintetico");
    let records = log
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).expect("record JSON valido"))
        .collect::<Vec<_>>();
    for function in [
        "aifa::active_ingredient_components",
        "aifa::form_stems",
        "aifa::dose_signatures",
        "aifa::json_string",
        "aifa::optional_string",
        "aifa::string_array",
        "aifa::aifa_error_detail_impl",
        "aifa::aifa_error_detail",
        "aifa::medication_fingerprint_impl",
        "aifa::medication_fingerprint",
        "aifa::confirm_aifa_candidate_impl",
        "aifa::confirm_aifa_candidate",
        "aifa::leaflet_download_dates_impl",
        "aifa::candidates_from_payload_impl",
        "aifa::candidates_from_payload",
        "aifa::ordered_candidates",
        "aifa::candidate_mismatches",
        "aifa::active_ingredients_match",
        "aifa::candidate_is_plausible",
        "aifa::commercial_names_match",
        "aifa::lookup_queries",
        "aifa::catalog_request",
        "aifa::lookup_candidate_search_with_impl",
        "aifa::lookup_candidate_search_with",
    ] {
        let (entry, exit) = assert_trace_span(&records, function);
        assert!(
            entry["arguments"].to_string().contains("secret_omitted"),
            "input derivato non omesso: {function}"
        );
        assert!(
            exit["result"].to_string().contains("secret_omitted"),
            "risultato derivato non omesso: {function}"
        );
    }
    assert!(!log.contains("synthetic-aifa-helper-secret"));
    fs::remove_dir_all(directory).expect("pulizia log sintetico");
}
