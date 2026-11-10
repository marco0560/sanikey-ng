use super::{
    AifaCandidate, AifaCandidateSearch, ConfirmedReferenceDecision, confirm_aifa_candidate,
    create_private_temp_file, download_confirmed_leaflets_with, leaflet_download_dates,
    local_leaflet_documents, lookup_candidate_search_with, medication_fingerprint,
    update_confirmed_reference, write_confirmed_references,
};
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

fn temporary_directory() -> PathBuf {
    let sequence = TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!("sanikey-ng-aifa-{}-{sequence}", std::process::id()))
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
