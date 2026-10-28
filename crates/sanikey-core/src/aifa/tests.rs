use super::{AifaCandidateSearch, lookup_candidate_search_with};
use crate::metadata::Medication;
use serde_json::{Value, json};

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
